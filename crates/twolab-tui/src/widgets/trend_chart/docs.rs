use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use super::{Series, TrendChart, human};
use crate::catalog::{ComponentDoc, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};

const USAGE: &str = r#"use twolab_tui::trend_chart::{Series, TrendChart};

// One value per day; None where nothing was recorded (a gap, not a zero).
let chart = TrendChart::new([
    Series::new("opus-4-8[1m]", opus_daily).note("41.2%"),
    Series::new("gpt-5.6-sol", gpt_daily)
        .low(gpt_samples.iter().map(|&n| n < 5))   // thin days drawn dim
        .note("30.1%"),
])
.x_labels(["10-23", "10-30", "11-05"])
.y_unit("t/s")
.y_format(|v| format!("{v:.0}"))
.low_note("dim = fewer than 5 samples")
.theme(theme);
frame.render_widget(&chart, area);"#;

const DAYS_14: [&str; 3] = ["10-23", "10-30", "11-05"];

/// Millions of tokens per day, 10-23 to 11-05; llmux was not running on
/// 10-31 and 11-01, so those days have no data at all.
const DAILY: [(&str, [Option<f64>; 14]); 4] = [
    (
        "opus-4-8[1m]",
        [
            Some(41.2),
            Some(44.8),
            Some(39.5),
            Some(52.1),
            Some(57.3),
            Some(31.0),
            Some(28.4),
            Some(49.9),
            None,
            None,
            Some(55.2),
            Some(61.7),
            Some(58.3),
            Some(63.4),
        ],
    ),
    (
        "gpt-5.6-sol",
        [
            Some(22.1),
            Some(25.3),
            Some(27.8),
            Some(24.4),
            Some(30.2),
            Some(18.9),
            Some(15.1),
            Some(29.6),
            None,
            None,
            Some(33.8),
            Some(31.2),
            Some(36.5),
            Some(34.9),
        ],
    ),
    (
        "fable-5",
        [
            Some(8.3),
            Some(12.4),
            Some(15.9),
            Some(14.1),
            Some(19.7),
            Some(9.8),
            Some(7.2),
            Some(21.5),
            None,
            None,
            Some(24.8),
            Some(22.3),
            Some(27.1),
            Some(29.6),
        ],
    ),
    (
        "grok-5",
        [
            Some(3.1),
            Some(2.4),
            Some(4.8),
            Some(6.2),
            Some(5.5),
            Some(1.9),
            Some(1.2),
            Some(7.4),
            None,
            None,
            Some(9.1),
            Some(8.6),
            Some(10.3),
            Some(11.8),
        ],
    ),
];

fn daily(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let total: f64 = DAILY.iter().flat_map(|(_, d)| d.iter().flatten()).sum();
    let series = DAILY.iter().map(|(name, days)| {
        let mine: f64 = days.iter().flatten().sum();
        Series::new(*name, days.iter().map(|d| d.map(|m| m * 1e6)))
            .note(format!("{:.1}%", mine * 100.0 / total))
    });
    TrendChart::new(series)
        .x_labels(DAYS_14)
        .theme(cx.theme)
        .render(area, buf);
}

/// End-to-end tokens/sec per day, with the day's sample count: under five
/// is a thin day, drawn dim; zero samples is no data.
const TPS: [(&str, [(f64, u32); 14]); 3] = [
    (
        "fable-5",
        [
            (0.0, 0),
            (0.0, 0),
            (0.0, 0),
            (0.0, 0),
            (151.0, 3),
            (163.0, 4),
            (172.0, 41),
            (168.0, 57),
            (176.0, 62),
            (181.0, 49),
            (174.0, 70),
            (179.0, 66),
            (185.0, 58),
            (183.0, 61),
        ],
    ),
    (
        "gpt-5.6-sol",
        [
            (104.0, 38),
            (109.0, 44),
            (98.0, 51),
            (112.0, 47),
            (118.0, 2),
            (121.0, 1),
            (107.0, 39),
            (115.0, 52),
            (0.0, 0),
            (111.0, 45),
            (119.0, 48),
            (116.0, 50),
            (122.0, 43),
            (117.0, 46),
        ],
    ),
    (
        "opus-4-8[1m]",
        [
            (71.0, 120),
            (68.0, 131),
            (74.0, 118),
            (66.0, 140),
            (72.0, 3),
            (63.0, 2),
            (69.0, 125),
            (75.0, 137),
            (73.0, 129),
            (77.0, 142),
            (70.0, 133),
            (76.0, 128),
            (78.0, 139),
            (74.0, 135),
        ],
    ),
];

fn tps(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let series = TPS.iter().map(|(name, days)| {
        Series::new(*name, days.iter().map(|&(v, n)| (n > 0).then_some(v)))
            .low(days.iter().map(|&(_, n)| n > 0 && n < 5))
    });
    TrendChart::new(series)
        .x_labels(DAYS_14)
        .y_format(|v| format!("{v:.0}"))
        .y_unit("t/s")
        .low_note("dim = fewer than 5 samples")
        .theme(cx.theme)
        .render(area, buf);
}

/// Requests per day through llmux, 10-07 to 11-05: a weekday rhythm on a
/// slow climb.
fn requests() -> Vec<Option<f64>> {
    const WEEK: [f64; 7] = [1.0, 1.1, 1.15, 1.05, 0.95, 0.45, 0.4];
    (0..30)
        .map(|i| Some(((2_400.0 + 25.0 * i as f64) * WEEK[i % 7]).round()))
        .collect()
}

fn single(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let days = requests();
    let total: f64 = days.iter().flatten().sum();
    TrendChart::new([Series::new("requests", days).note(format!("{} in 30d", human(total)))])
        .x_labels(["10-07", "10-22", "11-05"])
        .theme(cx.theme)
        .render(area, buf);
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "trend-chart",
    name: "Trend chart",
    ty: "twolab_tui::trend_chart::TrendChart",
    summary: "How each top series moved day by day.",
    description: "Braille lines for up to four series over dim axes with three labels each, and a one-line legend. A day with no data is a gap in the line, never a drop to zero; a day with too few samples is drawn dim, joins included, so a thin point never reads as a confident trend. Colours come from a four-hue theme ramp; without colour every sample carries its series' glyph (* + x o).",
    used_in: "llmux: tokens per day per model in the stats overlay, and observed tokens/sec per model in the perf overlay.",
    source: Source {
        project: "llmux",
        url: "https://github.com/2lab-ai/llmux/blob/524b6e4/src/tui/ui.rs#L1826-L1967",
        license: "MIT",
        how: "ported",
    },
    group: Group::Data,
    props: &[
        PropDoc {
            name: "TrendChart::new(series)",
            ty: "impl IntoIterator<Item = Series>",
            default: None,
            description: "Series in rank order; the first four are drawn, the rest counted in the legend.",
        },
        PropDoc {
            name: "Series::new(name, points).low(flags).note(s)",
            ty: "Series",
            default: None,
            description: "One Option<f64> per day (None = no data, a gap); per-day thin-sample flags; a legend note such as a share.",
        },
        PropDoc {
            name: ".x_labels([first, middle, last])",
            ty: "[&str; 3]",
            default: Some("none"),
            description: "Under the x axis; the middle one gives way first when narrow.",
        },
        PropDoc {
            name: ".y_format(f) / .y_unit(u)",
            ty: "fn(f64) -> String / &str",
            default: Some("human (48.2M) / none"),
            description: "How the y labels read; a unit after the top one.",
        },
        PropDoc {
            name: ".low_note(s)",
            ty: "&str",
            default: Some("\"dim = few samples\""),
            description: "Said in the legend when a thin sample is drawn.",
        },
        PropDoc {
            name: ".legend(on) / .empty(s)",
            ty: "bool / &str",
            default: Some("true / \"no data in this span\""),
            description: "The legend row; what to say when no series has a value.",
        },
        PropDoc {
            name: ".theme(t)",
            ty: "Theme",
            default: Some("Theme::ansi()"),
            description: "Series hues: accent, wait, done, fail (series_color); axes, labels and thin samples dim.",
        },
    ],
    keyboard: &[],
    a11y: &[
        "Under NO_COLOR each series marks every sample with its own glyph (* + x o), repeated in the legend.",
        "Thin samples are dim in every theme, and the legend says what dim means.",
        "No data is a gap, never a zero, so a missing day cannot read as a quiet one.",
        "Series beyond four, and legend entries that do not fit, are counted (+N more).",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "daily",
            title: "Tokens per day by model",
            description: "Four models over 14 days; llmux was off on 10-31 and 11-01, so every line has a gap there.",
            cols: 96,
            rows: 16,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(daily)),
        },
        DemoDoc {
            id: "tps",
            title: "Tokens/sec with thin days",
            description: "Observed speed; days with fewer than five samples are dim, days with none are gaps.",
            cols: 96,
            rows: 14,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(tps)),
        },
        DemoDoc {
            id: "single",
            title: "A single series",
            description: "Requests per day through llmux over 30 days.",
            cols: 80,
            rows: 12,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(single)),
        },
    ],
};
