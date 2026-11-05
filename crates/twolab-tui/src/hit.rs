//! What is under the pointer: rectangles recorded while drawing, looked up
//! when a click arrives.
//!
//! A widget pushes the rect it actually drew for each clickable thing (a tab,
//! a menu item, a row) into a [`HitMap`] during render; the host keeps the
//! map until the next frame and asks [`HitMap::hit_test`] on a mouse event.
//! Because the rects come from the real layout — in terminal cells, the
//! unit ratatui draws — a click can never land on a stale or miscounted
//! target.
//!
//! Generalised from llmux `src/tui/ui.rs` and `src/tui/mod.rs` (MIT, 2lab),
//! where nine hit structs (`TabHit`, `AccountRowHit`, `MenuChrome`, …) and
//! about eight inline `find(|h| row == h.area.y && …)` closures each assumed
//! one-row targets. Later pushes sit on top, the way llmux layers its input
//! line above the entry block.

use ratatui::layout::{Position, Rect};

/// Rects and the targets they stand for. The last pushed rect that contains
/// a point wins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitMap<T> {
    entries: Vec<(Rect, T)>,
}

impl<T> Default for HitMap<T> {
    fn default() -> Self {
        HitMap::new()
    }
}

impl<T> HitMap<T> {
    pub const fn new() -> HitMap<T> {
        HitMap {
            entries: Vec::new(),
        }
    }

    /// Record `target` at `area`. An empty rect is never hit, so it is not
    /// kept.
    pub fn push(&mut self, area: Rect, target: T) {
        if !area.is_empty() {
            self.entries.push((area, target));
        }
    }

    /// Record `target` at the part of `area` inside `visible` — for things
    /// drawn partly off screen or under a clipped viewport.
    pub fn push_clipped(&mut self, area: Rect, visible: Rect, target: T) {
        self.push(area.intersection(visible), target);
    }

    /// Forget every rect. Call it at the start of a frame so a click can
    /// never map onto last frame's layout.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The target under the cell at `(col, row)`, topmost first.
    pub fn hit_test(&self, col: u16, row: u16) -> Option<&T> {
        self.hit(col, row).map(|(_, t)| t)
    }

    /// The target under `(col, row)` with the rect it was recorded at —
    /// for targets that split themselves into zones (a run header whose
    /// first cells toggle and whose rest expands).
    pub fn hit(&self, col: u16, row: u16) -> Option<(Rect, &T)> {
        let p = Position::new(col, row);
        self.entries
            .iter()
            .rev()
            .find(|(r, _)| r.contains(p))
            .map(|(r, t)| (*r, t))
    }

    /// The rect a target was recorded at (the topmost, if pushed twice).
    pub fn area_of(&self, target: &T) -> Option<Rect>
    where
        T: PartialEq,
    {
        self.entries
            .iter()
            .rev()
            .find(|(_, t)| t == target)
            .map(|(r, _)| *r)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every recorded rect in push order (bottom first).
    pub fn iter(&self) -> impl Iterator<Item = (Rect, &T)> {
        self.entries.iter().map(|(r, t)| (*r, t))
    }
}

impl<T> Extend<(Rect, T)> for HitMap<T> {
    fn extend<I: IntoIterator<Item = (Rect, T)>>(&mut self, iter: I) {
        for (r, t) in iter {
            self.push(r, t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Tab {
        Dashboard,
        Usage,
        Logs,
    }

    /// A tab strip drawn left to right, each label's width in cells.
    fn tabs() -> HitMap<Tab> {
        let mut m = HitMap::new();
        let mut x = 0u16;
        for (tab, label) in [
            (Tab::Dashboard, " dashboard "),
            (Tab::Usage, " 사용량 "),
            (Tab::Logs, " logs "),
        ] {
            let w = crate::text::width(label) as u16;
            m.push(Rect::new(x, 1, w, 1), tab);
            x += w;
        }
        m
    }

    #[test]
    fn hits_by_cell_width_including_wide_labels() {
        let m = tabs();
        assert_eq!(m.hit_test(0, 1), Some(&Tab::Dashboard));
        assert_eq!(m.hit_test(10, 1), Some(&Tab::Dashboard));
        // " 사용량 " is 8 cells, not 5 chars.
        assert_eq!(m.hit_test(11, 1), Some(&Tab::Usage));
        assert_eq!(m.hit_test(18, 1), Some(&Tab::Usage));
        assert_eq!(m.hit_test(19, 1), Some(&Tab::Logs));
        assert_eq!(m.hit_test(24, 1), Some(&Tab::Logs));
        assert_eq!(m.hit_test(25, 1), None, "right edge is exclusive");
        assert_eq!(m.hit_test(3, 0), None, "row above");
        assert_eq!(m.hit_test(3, 2), None, "row below");
    }

    #[test]
    fn last_pushed_wins_where_rects_overlap() {
        let mut m = HitMap::new();
        m.push(Rect::new(0, 0, 40, 10), "entry block");
        m.push(Rect::new(2, 8, 30, 1), "input line");
        assert_eq!(m.hit_test(5, 8), Some(&"input line"));
        assert_eq!(m.hit_test(5, 7), Some(&"entry block"));
        assert_eq!(m.hit_test(35, 8), Some(&"entry block"));
        let (r, t) = m.hit(5, 8).expect("hit");
        assert_eq!((r, *t), (Rect::new(2, 8, 30, 1), "input line"));
    }

    #[test]
    fn multi_row_targets_hit_on_every_row() {
        let mut m = HitMap::new();
        m.push(Rect::new(0, 3, 20, 3), 7usize);
        for row in 3..6 {
            assert_eq!(m.hit_test(19, row), Some(&7));
        }
        assert_eq!(m.hit_test(0, 6), None);
    }

    #[test]
    fn empty_rects_are_dropped_and_clear_forgets() {
        let mut m = tabs();
        m.push(Rect::new(5, 5, 0, 1), Tab::Logs);
        m.push(Rect::new(5, 5, 3, 0), Tab::Logs);
        assert_eq!(m.len(), 3);
        assert_eq!(m.area_of(&Tab::Usage), Some(Rect::new(11, 1, 8, 1)));
        m.clear();
        assert!(m.is_empty());
        assert_eq!(m.hit_test(0, 1), None);
    }

    #[test]
    fn clipped_push_keeps_only_the_visible_part() {
        let mut m = HitMap::new();
        let viewport = Rect::new(0, 2, 30, 5);
        m.push_clipped(Rect::new(0, 0, 30, 4), viewport, "row");
        m.push_clipped(Rect::new(0, 9, 30, 1), viewport, "scrolled away");
        assert_eq!(m.hit_test(1, 1), None, "above the viewport");
        assert_eq!(m.hit_test(1, 2), Some(&"row"));
        assert_eq!(m.len(), 1);
        m.extend([(Rect::new(0, 6, 4, 1), "more")]);
        assert_eq!(
            m.iter().map(|(_, t)| *t).collect::<Vec<_>>(),
            ["row", "more"]
        );
    }
}
