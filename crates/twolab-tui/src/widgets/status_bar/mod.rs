// Derived from vercel-labs/fx `src/ui/render.zig` (`compactModelLabel`,
// `leadingPermissionModeFits`, `appendSessionStatusSegments`, `buildHintLine`)
// and `src/ui/footer/input_presentation.zig` (`composeHintRow`) at
// 412d841d73dd6e1b8de551022728d9c929caae17 (Apache-2.0, Copyright 2025
// Vercel, Inc.); translated from Zig to Rust. Rules and tests also lifted from
// 2lab-ai/xfx `src/tui/hint.rs` at 93f516b537adf52db208e3a343d9229c5f3d40e2
// (Apache-2.0, 2lab). Modified by 2lab: generic prioritised segments, a cue
// with variants, a note, honest `—` for unknowns, ratatui output.
//
// The fair-share fit is derived from herdrdev/herdr `src/ui/sidebar.rs`
// (`resolved_token_spans`) at ee5bfb5041bb3e4a49d591c72308aae3c81ecdde
// (Apache-2.0): flexible tokens keep one cell each, spare cells are dealt
// round-robin, each token is cut with `…`. Modified by 2lab: segments drop by
// priority rather than position when the minimums do not fit, the cut is by
// grapheme (`crate::text::truncate`), and the row can count what it dropped
// (`+N`) instead of losing it silently.

//! One row: what mode, which model, how full the context is, and what you
//! just armed.
//!
//! Ported from fx's hint row (`src/ui/render.zig`,
//! `src/ui/footer/input_presentation.zig`, Apache-2.0) by way of xfx's
//! `src/tui/hint.rs`. Segments are joined by ` · ` in the row's grey; on a
//! screen too narrow for all of them, whole segments go — the lowest
//! [`Segment::priority`] first, the rightmost of equals first — so what
//! survives is what the person needed most. Only the last segment standing is
//! ever cut, with `…`.
//!
//! **Fair share** (herdr's sidebar rows): a segment marked
//! [`Segment::flex`] gives way before anything is dropped. Each flexible
//! segment keeps at least one cell, the cells left over are dealt one at a
//! time, left to right, to the flexible segments that still want more, and
//! each one that does not get its whole text is cut with `…`. Whole segments
//! go (by priority, as above) only when even those minimums do not fit. A row
//! without flexible segments behaves exactly as described above.
//! [`StatusBar::count_dropped`] ends the left with `+N` when N segments were
//! dropped, so the person knows the row holds more than it shows.
//!
//! The right end holds one of two things, flush with the last column:
//!
//! - a **cue** ([`StatusBar::cue`]) about the next keystroke — "press esc
//!   again to interrupt". It is reserved before the left is laid out: it takes
//!   the widest of its variants that still leaves room for the most important
//!   segment, and the left drops segments to make room. When not even its
//!   shortest variant fits there, the cue takes the whole row — its `alone`
//!   text (fx's `esc esc to interrupt`) when that fits, else its widest variant
//!   that does — because a warning about a destructive key must not vanish.
//! - a **note** ([`StatusBar::note`]) — a status that is nice to have. It is
//!   shown only when it fits beside what the left kept.
//!
//! Unknowns are said, not invented: [`model_label`] and [`context_label`]
//! write `—` for what nobody measured, never `0`.
//!
//! The separator `·` (U+00B7) and `—` (U+2014) are East-Asian Ambiguous; a
//! host on a terminal that draws them wide passes an ASCII
//! [`StatusBar::separator`].

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::text;
use crate::theme::Theme;

/// What joins two segments (fx `render.zig`, `statusline_separator`).
pub const SEPARATOR: &str = " · ";

/// What an unknown value reads as.
pub const UNKNOWN: &str = "—";

/// Blank cells, at least, between the left and the right end.
const GAP: usize = 1;

/// One fact on the left of the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    /// `None` paints it in the row's style.
    pub style: Option<Style>,
    /// Higher stays longer. Among equals the rightmost goes first.
    pub priority: u8,
    /// Shrinks (to one cell, cut with `…`) before any segment is dropped.
    pub flex: bool,
}

impl Segment {
    /// A segment at priority 0, in the row's style.
    pub fn new(text: impl Into<String>) -> Segment {
        Segment {
            text: text.into(),
            style: None,
            priority: 0,
            flex: false,
        }
    }

    /// A segment that gives way first: it keeps at least one cell and gets
    /// its share of the spare ones, cut with `…` when that is not all of it.
    pub fn flex(mut self) -> Segment {
        self.flex = true;
        self
    }

    pub fn priority(mut self, priority: u8) -> Segment {
        self.priority = priority;
        self
    }

    pub fn style(mut self, style: Style) -> Segment {
        self.style = Some(style);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Cue {
    variants: Vec<String>,
    alone: Option<String>,
    style: Style,
}

/// The hint/status row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBar {
    segments: Vec<Segment>,
    separator: String,
    style: Style,
    cue: Option<Cue>,
    note: Option<(String, Style)>,
    count_dropped: bool,
}

/// What [`StatusBar::layout`] decided for one width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarLayout {
    /// Indices into the segments, in order, that are on the row.
    pub kept: Vec<usize>,
    /// The cells each kept segment is given, in the order of `kept`: its
    /// width, or less for a flexible (or the last) segment that is cut.
    pub widths: Vec<usize>,
    /// Segments with text that are not on the row.
    pub dropped: usize,
    /// The row ends its left side with `+N` ([`StatusBar::count_dropped`]).
    pub counted: bool,
    /// What the right end says, if anything.
    pub right: Option<String>,
    /// The cue's `alone` text took the whole row.
    pub alone: bool,
}

impl StatusBar {
    /// An empty row: segments in `theme.secondary()` (fx's status-line grey),
    /// a cue in `theme.accent()`, a note in `theme.dim()`.
    pub fn new(theme: Theme) -> StatusBar {
        StatusBar {
            segments: Vec::new(),
            separator: SEPARATOR.to_string(),
            style: theme.secondary(),
            cue: None,
            note: None,
            count_dropped: false,
        }
        .cue_style(theme.accent())
        .note_style(theme.dim())
    }

    pub fn segment(mut self, segment: Segment) -> StatusBar {
        self.segments.push(segment);
        self
    }

    pub fn segments(mut self, segments: impl IntoIterator<Item = Segment>) -> StatusBar {
        self.segments.extend(segments);
        self
    }

    /// The row's own style: segments without one, and the separators.
    pub fn style(mut self, style: Style) -> StatusBar {
        self.style = style;
        self
    }

    pub fn separator(mut self, separator: impl Into<String>) -> StatusBar {
        self.separator = separator.into();
        self
    }

    /// End the left with `+N` (after a separator) when N segments were
    /// dropped. Its room is kept before the segments are fitted.
    pub fn count_dropped(mut self, on: bool) -> StatusBar {
        self.count_dropped = on;
        self
    }

    /// Arm a cue: `variants` widest first, e.g. `["press esc again to
    /// interrupt", "esc again to interrupt", "esc esc"]`.
    pub fn cue<S: Into<String>>(mut self, variants: impl IntoIterator<Item = S>) -> StatusBar {
        let style = self.cue.as_ref().map_or_else(Style::new, |c| c.style);
        let variants: Vec<String> = variants
            .into_iter()
            .map(Into::into)
            .filter(|v| !v.is_empty())
            .collect();
        self.cue = Some(Cue {
            variants,
            alone: self.cue.take().and_then(|c| c.alone),
            style,
        });
        self
    }

    /// What the whole row says when no variant of the cue fits beside the
    /// most important segment.
    pub fn cue_alone(mut self, text: impl Into<String>) -> StatusBar {
        let cue = self.cue.get_or_insert_with(|| Cue {
            variants: Vec::new(),
            alone: None,
            style: Style::new(),
        });
        cue.alone = Some(text.into());
        self
    }

    pub fn cue_style(mut self, style: Style) -> StatusBar {
        match &mut self.cue {
            Some(cue) => cue.style = style,
            None => {
                self.cue = Some(Cue {
                    variants: Vec::new(),
                    alone: None,
                    style,
                })
            }
        }
        self
    }

    /// A right-aligned note, shown only when it fits beside the left. A cue
    /// outranks it.
    pub fn note(mut self, text: impl Into<String>) -> StatusBar {
        let style = self.note.as_ref().map_or_else(Style::new, |n| n.1);
        self.note = Some((text.into(), style));
        self
    }

    pub fn note_style(mut self, style: Style) -> StatusBar {
        let text = self.note.take().map(|n| n.0).unwrap_or_default();
        self.note = Some((text, style));
        self
    }

    fn armed(&self) -> Option<&Cue> {
        self.cue
            .as_ref()
            .filter(|c| !c.variants.is_empty() || c.alone.is_some())
    }

    fn seg_width(&self, i: usize) -> usize {
        text::width(&self.segments[i].text)
    }

    /// The cells a segment cannot go below: all of it, or one for a
    /// flexible one.
    fn least(&self, i: usize) -> usize {
        let w = self.seg_width(i);
        if self.segments[i].flex { w.min(1) } else { w }
    }

    /// The `+N` that ends the left, with its separator, if it is shown.
    fn count_width(&self, dropped: usize) -> usize {
        if self.count_dropped && dropped > 0 {
            text::width(&self.separator) + text::width(&format!("+{dropped}"))
        } else {
            0
        }
    }

    /// Drops segments, lowest priority (then rightmost) first, until the
    /// minimums of the rest fit `budget` or one is left; then deals the
    /// spare cells round-robin to the flexible ones (herdr
    /// `resolved_token_spans`).
    fn fit_left(&self, budget: usize) -> LeftFit {
        let mut kept: Vec<usize> = (0..self.segments.len())
            .filter(|&i| !self.segments[i].text.is_empty())
            .collect();
        let total = kept.len();
        if budget == 0 {
            return LeftFit {
                dropped: total,
                ..LeftFit::default()
            };
        }
        let sep = text::width(&self.separator);
        let need = |kept: &[usize]| -> usize {
            kept.iter().map(|&i| self.least(i)).sum::<usize>()
                + sep * kept.len().saturating_sub(1)
                + self.count_width(total - kept.len())
        };
        while kept.len() > 1 && need(&kept) > budget {
            let mut drop = 0;
            for (k, &i) in kept.iter().enumerate() {
                if self.segments[i].priority <= self.segments[kept[drop]].priority {
                    drop = k;
                }
            }
            kept.remove(drop);
        }
        let dropped = total - kept.len();
        let mut counted = self.count_dropped && dropped > 0;
        if let [only] = kept[..] {
            // The last one standing takes what is left beside the count,
            // and loses the count before its last cell.
            let mut room = budget.saturating_sub(self.count_width(dropped));
            if counted && room == 0 {
                counted = false;
                room = budget;
            }
            return LeftFit {
                widths: vec![self.seg_width(only).min(room)],
                kept,
                dropped,
                counted,
            };
        }
        let mut widths: Vec<usize> = kept.iter().map(|&i| self.least(i)).collect();
        let mut spare = budget.saturating_sub(need(&kept));
        while spare > 0 {
            let mut grew = false;
            for (k, &i) in kept.iter().enumerate() {
                if spare > 0 && self.segments[i].flex && widths[k] < self.seg_width(i) {
                    widths[k] += 1;
                    spare -= 1;
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        LeftFit {
            kept,
            widths,
            dropped,
            counted,
        }
    }

    /// The segment texts as drawn: each cut to its cells with `…`.
    fn drawn(&self, fit: &BarLayout) -> Vec<String> {
        fit.kept
            .iter()
            .zip(&fit.widths)
            .map(|(&i, &w)| text::truncate(&self.segments[i].text, w).into_owned())
            .collect()
    }

    /// The width of the segment that would survive alone.
    fn survivor_width(&self) -> usize {
        self.fit_left(1)
            .kept
            .first()
            .map_or(0, |&i| self.seg_width(i))
    }

    fn nonempty(&self) -> usize {
        self.segments.iter().filter(|s| !s.text.is_empty()).count()
    }

    /// A layout with no segments on the left.
    fn bare(&self, right: Option<String>, alone: bool) -> BarLayout {
        BarLayout {
            kept: Vec::new(),
            widths: Vec::new(),
            dropped: self.nonempty(),
            counted: false,
            right,
            alone,
        }
    }

    fn with_left(fit: LeftFit, right: Option<String>) -> BarLayout {
        BarLayout {
            kept: fit.kept,
            widths: fit.widths,
            dropped: fit.dropped,
            counted: fit.counted,
            right,
            alone: false,
        }
    }

    /// Cells the left takes as drawn: its segments, separators and `+N`.
    fn left_width(&self, layout: &BarLayout) -> usize {
        let sep = text::width(&self.separator);
        let segs: usize = self.drawn(layout).iter().map(|t| text::width(t)).sum();
        let count = if layout.counted {
            self.count_width(layout.dropped)
        } else {
            0
        };
        segs + sep * layout.kept.len().saturating_sub(1) + count
    }

    /// Which segments stay and what the right end says at `width` cells.
    pub fn layout(&self, width: u16) -> BarLayout {
        let w = usize::from(width);
        if let Some(cue) = self.armed() {
            let survivor = self.survivor_width();
            let room = |v: &str| {
                let need = if survivor == 0 { 0 } else { survivor + GAP };
                text::width(v) + need <= w
            };
            let Some(right) = cue.variants.iter().find(|v| room(v)) else {
                // Not even the shortest variant fits beside the most
                // important segment: the cue takes the row. Its `alone` text
                // when that fits, else the widest variant that fits, flush
                // right; else `alone` (or the last variant) cut with `…`.
                if let Some(alone) = cue.alone.as_ref().filter(|a| text::width(a) <= w) {
                    return self.bare(Some(alone.clone()), true);
                }
                if let Some(v) = cue.variants.iter().find(|v| text::width(v) <= w) {
                    return self.bare(Some(v.clone()), false);
                }
                return self.bare(
                    cue.alone.clone().or_else(|| cue.variants.last().cloned()),
                    true,
                );
            };
            let right = right.clone();
            let budget = w.saturating_sub(text::width(&right) + GAP);
            return StatusBar::with_left(self.fit_left(budget), Some(right));
        }
        let mut layout = StatusBar::with_left(self.fit_left(w), None);
        let used = self.left_width(&layout);
        layout.right = self
            .note
            .as_ref()
            .map(|n| &n.0)
            .filter(|n| !n.is_empty())
            .filter(|n| {
                let gap = if layout.kept.is_empty() { 0 } else { GAP };
                used + gap + text::width(n) <= w
            })
            .cloned();
        layout
    }

    /// The row as one line, at most `width` cells.
    pub fn line(&self, width: u16) -> Line<'static> {
        let w = usize::from(width);
        let layout = self.layout(width);
        let right_style = match (self.armed(), &self.note) {
            (Some(cue), _) => cue.style,
            (None, Some((_, style))) => *style,
            (None, None) => self.style,
        };
        if layout.alone {
            let alone = layout.right.unwrap_or_default();
            return Line::from(Span::styled(
                text::truncate(&alone, w).into_owned(),
                right_style,
            ));
        }
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(layout.kept.len() * 2 + 4);
        for (n, (&i, shown)) in layout.kept.iter().zip(self.drawn(&layout)).enumerate() {
            if n > 0 {
                spans.push(Span::styled(self.separator.clone(), self.style));
            }
            let seg = &self.segments[i];
            spans.push(Span::styled(shown, seg.style.unwrap_or(self.style)));
        }
        if layout.counted {
            if !layout.kept.is_empty() {
                spans.push(Span::styled(self.separator.clone(), self.style));
            }
            spans.push(Span::styled(format!("+{}", layout.dropped), self.style));
        }
        let right = layout.right.filter(|r| !r.is_empty());
        let right_w = right.as_deref().map_or(0, text::width);
        let left_budget = match right {
            Some(_) => w.saturating_sub(right_w + GAP),
            None => w,
        };
        // Every segment is already cut to its cells; this only guards.
        let mut line = text::truncate_line(Line::from(spans), left_budget);
        if let Some(right) = right {
            let used = text::line_width(&line);
            let pad = w.saturating_sub(used + right_w);
            if pad > 0 {
                line.spans.push(Span::raw(" ".repeat(pad)));
            }
            line.spans.push(Span::styled(right, right_style));
            line = text::truncate_line(line, w);
        }
        line
    }
}

/// What the left side keeps at one budget.
#[derive(Debug, Clone, Default)]
struct LeftFit {
    kept: Vec<usize>,
    widths: Vec<usize>,
    dropped: usize,
    counted: bool,
}

impl StatusBar {
    /// Where each segment that [`StatusBar::line`] keeps is drawn in `area`'s
    /// first row: `(segment index, rect)`, left to right, clipped to what is
    /// visible. A host that makes segments clickable (llmux's settings bar)
    /// pushes these into a [`crate::hit::HitMap`]; a dropped segment has no
    /// rect, so a click can never land on something that is not there. A
    /// segment cut to share the row gets the cells it is drawn in.
    pub fn hits(&self, area: Rect) -> Vec<(usize, Rect)> {
        if area.is_empty() {
            return Vec::new();
        }
        let layout = self.layout(area.width);
        if layout.alone {
            return Vec::new();
        }
        let w = usize::from(area.width);
        let right_w = layout
            .right
            .as_deref()
            .filter(|r| !r.is_empty())
            .map_or(0, text::width);
        let budget = if right_w > 0 {
            w.saturating_sub(right_w + GAP)
        } else {
            w
        };
        let sep = text::width(&self.separator);
        let mut x = 0usize;
        let mut out = Vec::with_capacity(layout.kept.len());
        for (n, (&i, shown)) in layout.kept.iter().zip(self.drawn(&layout)).enumerate() {
            if n > 0 {
                x += sep;
            }
            let seg_w = text::width(&shown).min(budget.saturating_sub(x));
            if seg_w == 0 {
                break;
            }
            out.push((
                i,
                Rect {
                    x: area.x + x as u16,
                    y: area.y,
                    width: seg_w as u16,
                    height: 1,
                },
            ));
            x += seg_w;
        }
        out
    }
}

impl Widget for &StatusBar {
    /// Draws on the area's first row.
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        buf.set_line(area.x, area.y, &self.line(area.width), area.width);
    }
}

impl Widget for StatusBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

/// A model id as the row names it (fx `compactModelLabel`): the provider
/// prefix goes, and a Claude id loses `claude-` and spaces its family from
/// its version — `anthropic/claude-opus-4-7` is `opus 4-7`. Anything else is
/// its bare name.
pub fn compact_model(id: &str) -> String {
    let bare = id.rsplit('/').next().unwrap_or(id);
    let Some(family) = bare.strip_prefix("claude-") else {
        return bare.to_string();
    };
    for (prefix, label) in [
        ("opus-", "opus "),
        ("sonnet-", "sonnet "),
        ("haiku-", "haiku "),
    ] {
        if let Some(version) = family.strip_prefix(prefix) {
            return format!("{label}{version}");
        }
    }
    family.to_string()
}

/// [`compact_model`], or `model —` when the model is not known yet.
pub fn model_label(id: Option<&str>) -> String {
    match id.map(compact_model) {
        Some(label) if !label.is_empty() => label,
        _ => format!("model {UNKNOWN}"),
    }
}

/// How much of the context window is spent (fx `render.zig`):
/// `126k/200k 63%`, in thousands with integer maths. `12k/—` when nobody
/// published the window, `context —` when nothing was measured. A window of
/// zero reads as 0 %, not a division.
pub fn context_label(used: Option<u64>, window: Option<u64>) -> String {
    let Some(used) = used else {
        return format!("context {UNKNOWN}");
    };
    let Some(window) = window else {
        return format!("{}k/{UNKNOWN}", used / 1000);
    };
    let percent = used.saturating_mul(100).checked_div(window).unwrap_or(0);
    format!("{}k/{}k {percent}%", used / 1000, window / 1000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::{Color, Modifier};

    /// xfx's test row: `ask · opus 4-7`, with the meter when given.
    fn bar(context: Option<&str>) -> StatusBar {
        let mut b = StatusBar::new(Theme::ansi())
            .segment(Segment::new("ask").priority(1))
            .segment(Segment::new(compact_model("anthropic/claude-opus-4-7")).priority(2));
        if let Some(c) = context {
            b = b.segment(Segment::new(c));
        }
        b
    }

    fn text_of(bar: &StatusBar, w: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, 1));
        bar.render(buf.area, &mut buf);
        snapshot::text(&buf)
    }

    #[test]
    fn the_model_label_drops_the_provider_and_the_family_prefix() {
        assert_eq!(compact_model("anthropic/claude-opus-4.7"), "opus 4.7");
        assert_eq!(compact_model("anthropic/claude-opus-4-7"), "opus 4-7");
        assert_eq!(compact_model("anthropic/claude-sonnet-4.5"), "sonnet 4.5");
        assert_eq!(compact_model("anthropic/claude-haiku-4.5"), "haiku 4.5");
        assert_eq!(compact_model("anthropic/claude-3-5-turbo"), "3-5-turbo");
        assert_eq!(compact_model("openai/gpt-5.6-sol"), "gpt-5.6-sol");
        assert_eq!(compact_model("a/b/zai/glm-5.2"), "glm-5.2");
        assert_eq!(compact_model("plain"), "plain");
    }

    #[test]
    fn unknowns_are_said_not_invented() {
        assert_eq!(model_label(None), "model —");
        assert_eq!(model_label(Some("anthropic/")), "model —");
        assert_eq!(model_label(Some("openai/gpt-5.6-sol")), "gpt-5.6-sol");
        assert_eq!(context_label(Some(126_000), Some(200_000)), "126k/200k 63%");
        assert_eq!(context_label(Some(12_345), Some(200_000)), "12k/200k 6%");
        assert_eq!(context_label(Some(12_345), None), "12k/—");
        assert_eq!(context_label(None, Some(200_000)), "context —");
        // A window of zero is a percentage of nothing, not a panic.
        assert_eq!(context_label(Some(12_000), Some(0)), "12k/0k 0%");
        assert_eq!(
            context_label(Some(u64::MAX), Some(1)),
            format!("{}k/0k {}%", u64::MAX / 1000, u64::MAX)
        );
    }

    #[test]
    fn segments_are_joined_in_order_by_the_separator() {
        let b = bar(Some("12k/200k 6%"));
        assert_eq!(text_of(&b, 200), "ask · opus 4-7 · 12k/200k 6%");
        assert_eq!(
            text_of(&b.clone().separator(" | "), 200),
            "ask | opus 4-7 | 12k/200k 6%"
        );
    }

    #[test]
    fn a_narrow_row_drops_the_lowest_priority_first_and_never_overflows() {
        let b = bar(Some("12k/200k 6%"));
        // xfx: at twenty the meter is gone and the mode stays; at twelve the
        // mode has given way in front of the model.
        assert_eq!(text_of(&b, 20), "ask · opus 4-7");
        assert_eq!(text_of(&b, 12), "opus 4-7");
        // The last segment standing is cut, with an ellipsis.
        assert_eq!(text_of(&b, 6), "opus …");
        for w in 0..60u16 {
            assert!(text::line_width(&b.line(w)) <= usize::from(w), "{w}");
        }
    }

    #[test]
    fn hit_rects_follow_the_kept_segments() {
        let bar = StatusBar::new(Theme::ansi())
            .segment(Segment::new("f filter").priority(3))
            .segment(Segment::new("m mode").priority(1))
            .segment(Segment::new("e effort").priority(2));
        let area = Rect::new(2, 5, 40, 1);
        let hits = bar.hits(area);
        let line = text_of(&bar, 40);
        assert_eq!(line, "f filter · m mode · e effort");
        assert_eq!(
            hits,
            vec![
                (0, Rect::new(2, 5, 8, 1)),
                (1, Rect::new(13, 5, 6, 1)),
                (2, Rect::new(22, 5, 8, 1)),
            ]
        );
        // Each rect covers exactly its segment's text on the drawn row.
        for (i, r) in &hits {
            let x0 = usize::from(r.x - area.x);
            let drawn: String = line.chars().skip(x0).take(usize::from(r.width)).collect();
            assert_eq!(drawn, ["f filter", "m mode", "e effort"][*i]);
        }
        // Narrow: `m mode` (lowest) is dropped and has no rect.
        let narrow = bar.hits(Rect::new(0, 0, 20, 1));
        assert_eq!(narrow.iter().map(|h| h.0).collect::<Vec<_>>(), vec![0, 2]);
    }

    #[test]
    fn equal_priorities_drop_from_the_right() {
        let b = StatusBar::new(Theme::ansi())
            .segment(Segment::new("one"))
            .segment(Segment::new("two"))
            .segment(Segment::new("three"));
        assert_eq!(text_of(&b, 15), "one · two");
        assert_eq!(b.layout(15).kept, vec![0, 1]);
        assert_eq!(text_of(&b, 8), "one");
    }

    #[test]
    fn an_empty_segment_takes_no_separator_with_it() {
        let b = StatusBar::new(Theme::ansi())
            .segment(Segment::new("ask"))
            .segment(Segment::new(""))
            .segment(Segment::new("gpt-5"));
        assert_eq!(text_of(&b, 40), "ask · gpt-5");
    }

    #[test]
    fn a_cue_is_flush_with_the_last_column() {
        let b = bar(None).cue(["esc again to clear"]);
        for w in [40u16, 80, 120] {
            let line = b.line(w);
            assert_eq!(text::line_width(&line), usize::from(w), "{w}");
            let s = text_of(&b, w);
            assert!(s.starts_with("ask · opus 4-7 "), "{s:?}");
            assert!(s.ends_with("esc again to clear"), "{s:?}");
        }
    }

    #[test]
    fn a_cue_survives_a_row_too_narrow_for_the_segments() {
        let b = bar(Some("12k/200k 6%")).cue(["esc again to clear"]);
        let s = text_of(&b, 24);
        assert!(s.ends_with("esc again to clear"), "{s:?}");
        for w in 0..40u16 {
            assert!(text::line_width(&b.line(w)) <= usize::from(w), "{w}");
        }
    }

    #[test]
    fn the_interrupt_cue_shrinks_then_takes_the_row() {
        let b = bar(Some("12k/200k 6%"))
            .cue([
                "press esc again to interrupt",
                "esc again to interrupt",
                "esc esc interrupt",
                "esc esc",
            ])
            .cue_alone("esc esc to interrupt");
        let wide = text_of(&b, 80);
        assert!(wide.starts_with("ask · opus 4-7 · 12k/200k 6%"), "{wide:?}");
        assert!(wide.ends_with("press esc again to interrupt"), "{wide:?}");
        // The left gives way before the cue loses its words...
        assert_eq!(text_of(&b, 40), "opus 4-7    press esc again to interrupt");
        // ...then the cue shortens, keeping the most important segment...
        assert_eq!(
            b.layout(32).right.as_deref(),
            Some("esc again to interrupt")
        );
        assert_eq!(text_of(&b, 18), "opus 4-7   esc esc");
        // ...and when none fits beside it, the cue owns the row: `alone`
        // when it fits, else the widest variant that does, flush right.
        assert_eq!(
            b.layout(16).kept,
            vec![1],
            "esc esc beside opus 4-7 is sixteen cells"
        );
        let lone = b
            .clone()
            .segment(Segment::new("a-very-long-model-name").priority(9));
        let alone = lone.layout(22);
        assert!(alone.alone && alone.kept.is_empty());
        assert_eq!(text_of(&lone, 22), "esc esc to interrupt");
        let narrow = b.layout(15);
        assert!(!narrow.alone && narrow.kept.is_empty());
        assert_eq!(text_of(&b, 15), "        esc esc");
        assert_eq!(text_of(&b, 5), "esc …");
        for w in 0..90u16 {
            assert!(text::line_width(&lone.line(w)) <= usize::from(w), "{w}");
        }
    }

    #[test]
    fn a_note_shows_only_beside_what_the_left_kept() {
        let b = bar(None).note("subagent 2/3");
        assert!(text_of(&b, 40).ends_with("subagent 2/3"));
        assert_eq!(text::line_width(&b.line(40)), 40);
        // Too narrow for both: the left keeps its segments, the note goes.
        assert_eq!(text_of(&b, 20), "ask · opus 4-7");
        // A cue outranks a note.
        let armed = b.clone().cue(["esc again to clear"]);
        assert!(text_of(&armed, 60).ends_with("esc again to clear"));
        assert!(!text_of(&armed, 60).contains("subagent"));
    }

    /// herdr's sidebar row: a fixed state word, then agent, workspace and
    /// branch, which give way.
    fn agent_row() -> StatusBar {
        StatusBar::new(Theme::ansi())
            .segment(Segment::new("working").priority(9))
            .segment(Segment::new("claude").flex())
            .segment(Segment::new("llmux-dashboard").flex())
            .segment(Segment::new("feat/quota-poller-backoff").flex())
    }

    #[test]
    fn flexible_segments_keep_a_cell_and_share_the_rest_round_robin() {
        let b = agent_row();
        assert_eq!(
            text_of(&b, 62),
            "working · claude · llmux-dashboard · feat/quota-poller-backoff"
        );
        // 24 cells for three flexible segments: one each, then one at a time
        // left to right until `claude` is whole and the other two split the rest.
        assert_eq!(b.layout(40).widths, vec![7, 6, 9, 9]);
        assert_eq!(text_of(&b, 40), "working · claude · llmux-da… · feat/quo…");
        assert_eq!(text_of(&b, 20), "working · c… · … · …");
        assert_eq!(
            b.layout(20).dropped,
            0,
            "nothing goes while the minimums fit"
        );
        // The minimums no longer fit: a whole segment goes, by priority.
        let narrow = b.layout(18);
        assert_eq!((narrow.kept.clone(), narrow.dropped), (vec![0, 1, 2], 1));
        assert_eq!(text_of(&b, 18), "working · cl… · l…");
        for w in 0..80u16 {
            let l = b.layout(w);
            assert!(text::line_width(&b.line(w)) <= usize::from(w), "{w}");
            assert_eq!(l.kept.len(), l.widths.len());
            assert_eq!(l.kept.len() + l.dropped, 4, "{w}");
            if l.kept.len() > 1 {
                assert!(l.widths.iter().all(|&c| c >= 1), "{w}: {l:?}");
            }
        }
    }

    #[test]
    fn dropped_segments_can_be_counted() {
        let b = agent_row().count_dropped(true);
        assert_eq!(text_of(&b, 18), "working · cl… · +2");
        let l = b.layout(18);
        assert!(l.counted && l.dropped == 2);
        // Without flexible segments too: the count's room is kept first.
        let b = bar(Some("12k/200k 6%")).count_dropped(true);
        assert_eq!(text_of(&b, 40), "ask · opus 4-7 · 12k/200k 6%");
        assert!(!b.layout(40).counted, "nothing dropped, nothing counted");
        assert_eq!(text_of(&b, 20), "ask · opus 4-7 · +1");
        assert_eq!(text_of(&b, 12), "opus 4… · +2");
        // The last segment keeps its last cell before the count does.
        assert_eq!(text_of(&b, 5), "opus…");
        assert!(!b.layout(5).counted);
        for w in 0..60u16 {
            assert!(text::line_width(&b.line(w)) <= usize::from(w), "{w}");
        }
        // Off by default: today's row is unchanged.
        assert_eq!(text_of(&bar(Some("12k/200k 6%")), 20), "ask · opus 4-7");
    }

    #[test]
    fn hit_rects_cover_what_a_shared_segment_draws() {
        let b = agent_row().count_dropped(true);
        for w in [62u16, 40, 20, 18, 9] {
            let area = Rect::new(3, 2, w, 1);
            let mut buf = Buffer::empty(Rect::new(0, 0, 70, 4));
            (&b).render(area, &mut buf);
            let row = &snapshot::lines(&buf)[2];
            let l = b.layout(w);
            let hits = b.hits(area);
            assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), l.kept, "{w}");
            for (i, r) in &hits {
                assert!(r.right() <= area.right(), "{w}: inside the area");
                let drawn: String = row
                    .chars()
                    .skip(usize::from(r.x))
                    .take(usize::from(r.width))
                    .collect();
                let want = text::truncate(&b.segments[*i].text, usize::from(r.width));
                assert_eq!(drawn, want, "{w}: segment {i}");
            }
        }
    }

    /// herdr `conditional_style_survives_truncation_and_removes_theme_modifiers`
    /// and the truncation probes of H5: a shared cut keeps the segment's style
    /// and never splits a grapheme.
    #[test]
    fn a_shared_cut_keeps_its_style_and_whole_graphemes() {
        let red = Style::new().fg(Color::Red);
        for w in [4u16, 40] {
            let b = StatusBar::new(Theme::ansi())
                .segment(Segment::new("long-workspace-name").flex().style(red));
            let line = b.line(w);
            assert!(text::line_width(&line) <= usize::from(w));
            assert!(line.spans.iter().all(|s| s.style == red), "{line:?}");
        }
        let b = StatusBar::new(Theme::ansi())
            .segment(Segment::new("\u{1f1f0}\u{1f1f7} team").flex())
            .segment(Segment::new("👍🏽ok").flex())
            .segment(Segment::new("👩\u{200d}💻👩\u{200d}💻").flex());
        // 9 cells: separators take 6, one cell each.
        assert_eq!(text_of(&b, 9), "… · … · …");
        // 15: three cells each — the flag stays whole, the thumbs-up keeps
        // its skin tone, the coder keeps its joiner (herdr: `🇰…`, `👍…`,
        // `👩\u{200d}…`).
        let row = b.line(15);
        let s: String = row.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(s, "\u{1f1f0}\u{1f1f7}… · 👍🏽… · 👩\u{200d}💻…");
        assert_eq!(text::line_width(&row), 15);
    }

    #[test]
    fn a_cue_still_owns_its_end_beside_shared_segments() {
        let b = agent_row().cue(["esc again to clear", "esc esc"]);
        for w in [30u16, 40, 60] {
            let s = text_of(&b, w);
            assert!(s.starts_with("working"), "{s:?}");
            assert!(
                s.ends_with("esc again to clear") || s.ends_with("esc esc"),
                "{s:?}"
            );
            assert_eq!(text::line_width(&b.line(w)), usize::from(w), "{w}");
        }
    }

    #[test]
    fn styles_come_from_the_theme_and_the_segment() {
        let t = Theme::dark();
        let b = StatusBar::new(t)
            .segment(Segment::new("auto").style(t.text()))
            .segment(Segment::new("gpt-5"))
            .cue(["esc esc"]);
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 1));
        b.render(buf.area, &mut buf);
        assert_eq!(buf[(0, 0)].fg, t.text, "a lifted mode");
        assert_eq!(buf[(5, 0)].fg, t.text_secondary, "the separator");
        assert_eq!(buf[(7, 0)].fg, t.text_secondary, "the row's grey");
        assert_eq!(buf[(29, 0)].fg, t.accent, "the cue");
        let mono = Theme::mono();
        let b = StatusBar::new(mono)
            .segment(Segment::new("auto").style(mono.text()))
            .note("ok");
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 1));
        b.render(buf.area, &mut buf);
        for cell in buf.content() {
            assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset));
        }
        assert!(
            buf[(29, 0)].modifier.contains(Modifier::DIM),
            "the note is dim"
        );
    }
}
