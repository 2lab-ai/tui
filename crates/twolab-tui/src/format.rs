//! How long, how much, how long ago — each in a fixed small shape.
//!
//! Pure formatters: every function takes its inputs explicitly — the
//! duration, the instant, `now`, the UTC offset — and never reads a clock or
//! the machine's time zone. The host reads the clock once per frame and its
//! own zone offset (llmux does it with `localtime_r`) and passes both in.
//!
//! Ported from llmux `src/tui/format.rs` (MIT, 2lab), with the cost and count
//! cells from `src/tui/ui.rs` (`format_cost`, `group_thousands`,
//! `usage_cost_parts`, `tps_cell`, `opt_count`) and `compact_duration` from
//! `src/scheduler/select.rs`. Changes on the way:
//! - one civil-date routine ([`civil_from_days`]); llmux's
//!   `absolute_utc_label` carried a second, inline copy;
//! - the absolute stamp and the wall clock take an offset (llmux pinned
//!   them to UTC), so `0` gives the old UTC labels;
//! - honest empties: a non-finite percent or cost reads `—`, never `NaN%`;
//! - carries no longer overflow a unit: `999_999` tokens read `1.0M` (llmux:
//!   `1000.0k`) and `$0.99996` reads `$1.00` (llmux: `$1.0000`).
//!
//! Name map: `elapsed_secs` → [`elapsed`], `age_unit` → [`age`],
//! `clock_hms_utc` → [`clock_hms`], `absolute_utc_label` → [`absolute_stamp`],
//! `client_short_name` → [`short_name`], `format_cost` → [`cost`],
//! `usage_cost_parts` → [`cost_parts`], `tps_cell` → [`tps`].

use std::time::{Duration, SystemTime};

/// What an unknown value reads as. Never a made-up `0`.
pub const UNKNOWN: &str = "—";

const DAY: u64 = 86_400;
const HOUR: u64 = 3_600;
const MINUTE: u64 = 60;

/// `(days, hours, minutes, seconds)` of a whole-second duration.
fn units(d: Duration) -> (u64, u64, u64, u64) {
    let total = d.as_secs();
    (
        total / DAY,
        (total % DAY) / HOUR,
        (total % HOUR) / MINUTE,
        total % MINUTE,
    )
}

/// Compact countdown, the largest two units: `2d 4h`, `2h 13m`, `13m 05s`,
/// `42s`.
pub fn countdown(remaining: Duration) -> String {
    match countdown_units(remaining) {
        (head, Some(tail)) => format!("{head} {tail}"),
        (head, None) => head,
    }
}

/// [`countdown`] split in two so a renderer can embolden the larger unit,
/// which carries most of the signal: `("7d", Some("10h"))`,
/// `("10h", Some("15m"))`, `("17m", Some("35s"))`, `("42s", None)`.
pub fn countdown_units(remaining: Duration) -> (String, Option<String>) {
    let (days, hours, mins, secs) = units(remaining);
    if days > 0 {
        (format!("{days}d"), Some(format!("{hours}h")))
    } else if hours > 0 {
        (format!("{hours}h"), Some(format!("{mins:02}m")))
    } else if mins > 0 {
        (format!("{mins}m"), Some(format!("{secs:02}s")))
    } else {
        (format!("{secs}s"), None)
    }
}

/// The widest cell count [`countdown`] can take: `23h 59m` / `59m 59s`.
/// Day counts of three or more digits (`100d 0h`) are also seven cells.
pub const COUNTDOWN_MAX_WIDTH: usize = 7;

/// Countdown to a deadline as `Nd Nh Nm Ns`. Leading zero units are left
/// out, but once a unit appears every smaller one follows, and seconds always
/// show (an expired deadline reads `0s`).
pub fn remaining_hms(remaining: Duration) -> String {
    let (days, hours, mins, secs) = units(remaining);
    let mut out = String::with_capacity(16);
    if days > 0 {
        out.push_str(&format!("{days}d "));
    }
    if hours > 0 || !out.is_empty() {
        out.push_str(&format!("{hours}h "));
    }
    if mins > 0 || !out.is_empty() {
        out.push_str(&format!("{mins}m "));
    }
    out.push_str(&format!("{secs}s"));
    out
}

/// The widest form [`remaining_hms`] takes while its leading unit stays the
/// same — `remaining_hms_bound(3d …)` is the width of `3d 23h 59m 59s`. A
/// layout that fits this does not jump every second.
pub fn remaining_hms_bound(remaining: Duration) -> usize {
    let (days, hours, mins, _) = units(remaining);
    if days > 0 {
        days.to_string().len() + "d 23h 59m 59s".len()
    } else if hours > 0 {
        "23h 59m 59s".len()
    } else if mins > 0 {
        "59m 59s".len()
    } else {
        "59s".len()
    }
}

/// Compact countdown without the space: `2d4h`, `6h52m`, `3m12s`, `42s` —
/// for reasons in a status column ("cooldown 3m12s").
pub fn compact_duration(d: Duration) -> String {
    let (days, hours, mins, secs) = units(d);
    if days > 0 {
        format!("{days}d{hours}h")
    } else if hours > 0 {
        format!("{hours}h{mins:02}m")
    } else if mins > 0 {
        format!("{mins}m{secs:02}s")
    } else {
        format!("{secs}s")
    }
}

/// Elapsed seconds with one decimal, for a request in flight: `3.2s`.
pub fn elapsed(d: Duration) -> String {
    format!("{:.1}s", d.as_secs_f64())
}

/// The single largest unit, for "refreshed N ago": `42s`, `3m`, `6h`, `2d`.
/// Coarser than [`compact_duration`] on purpose — an age needs magnitude,
/// not precision.
pub fn age(d: Duration) -> String {
    let total = d.as_secs();
    if total >= DAY {
        format!("{}d", total / DAY)
    } else if total >= HOUR {
        format!("{}h", total / HOUR)
    } else if total >= MINUTE {
        format!("{}m", total / MINUTE)
    } else {
        format!("{total}s")
    }
}

/// `↻3m`: how long ago something was refreshed. `None` when it never was.
/// A refresh in the future (clock skew) reads `↻0s`.
pub fn refreshed_marker(last_refresh: Option<SystemTime>, now: SystemTime) -> Option<String> {
    let ago = now.duration_since(last_refresh?).unwrap_or_default();
    Some(format!("\u{21bb}{}", age(ago)))
}

/// Whole-number percentage of a fraction, clamped to 0–100: `42%`. A
/// non-finite fraction reads `—`.
pub fn percent(fraction: f64) -> String {
    if fraction.is_finite() {
        format!("{:.0}%", fraction.clamp(0.0, 1.0) * 100.0)
    } else {
        UNKNOWN.to_string()
    }
}

/// A count in at most six cells: `999`, `1.2k`, `243.6k`, `3.4M`, `243.6G`,
/// up through `T`, `P` and `E` (`u64::MAX` reads `18.4E`). A value that would
/// round up to `1000.0` of one unit moves to the next (`999_999` → `1.0M`).
pub fn human_count(n: u64) -> String {
    const UNITS: [(f64, &str); 6] = [
        (1e3, "k"),
        (1e6, "M"),
        (1e9, "G"),
        (1e12, "T"),
        (1e15, "P"),
        (1e18, "E"),
    ];
    if n < 1_000 {
        return n.to_string();
    }
    let v = n as f64;
    let (scale, unit) = UNITS
        .iter()
        .copied()
        .find(|(scale, _)| v / scale < 999.95)
        .unwrap_or(UNITS[UNITS.len() - 1]);
    format!("{:.1}{unit}", v / scale)
}

/// [`human_count`], or `—` when the upstream never reported the number.
pub fn opt_count(v: Option<u64>) -> String {
    v.map_or_else(|| UNKNOWN.to_string(), human_count)
}

/// Tokens per second: `45t/s`, `7.2t/s` below ten, `—` without a sample.
pub fn tps(tps: Option<f64>) -> String {
    match tps.filter(|t| t.is_finite()) {
        Some(t) if t >= 10.0 => format!("{t:.0}t/s"),
        Some(t) => format!("{t:.1}t/s"),
        None => UNKNOWN.to_string(),
    }
}

/// A US-dollar amount: `$3.78` from a dollar up, `$0.0123` below (small
/// per-request costs stay legible), `$0.0000` for exactly zero. A negative
/// or non-finite amount reads `—`: invalid data never dresses up as a price.
pub fn cost(usd: f64) -> String {
    if !usd.is_finite() || usd < 0.0 {
        return UNKNOWN.to_string();
    }
    if usd < 1.0 {
        let fine = format!("${usd:.4}");
        if fine.starts_with("$0.") {
            return fine;
        }
    }
    format!("${usd:.2}")
}

/// A non-negative amount split into its grouped integer part and its
/// fraction, for a ledger column: `("1,234", ".50")`, `("0", ".0076")`.
/// Sub-dollar amounts keep four fraction digits, larger ones two. Rounding
/// is done on whole sub-units, so a carry never makes an eleven-digit
/// fraction (`0.99999` → `("1", ".00")`). Negative input reads as zero;
/// filter it first.
pub fn cost_parts(cost: f64) -> (String, String) {
    let (dollars, frac) = if cost < 0.995 {
        let ten_thousandths = (cost * 10_000.0).round() as u64;
        (
            ten_thousandths / 10_000,
            format!(".{:04}", ten_thousandths % 10_000),
        )
    } else {
        let cents = (cost * 100.0).round() as u64;
        (cents / 100, format!(".{:02}", cents % 100))
    };
    (group_thousands(dollars), frac)
}

/// `1234567` → `1,234,567`. A money column groups; it never compresses to
/// `1.2M` the way [`human_count`] does.
pub fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Seconds since the epoch of the offset-shifted wall clock. Instants
/// before 1970 read as the epoch.
fn local_secs(at: SystemTime, offset_secs: i64) -> i64 {
    let epoch = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    epoch.saturating_add(offset_secs)
}

/// `(month, day, seconds into the day)` of the local wall clock.
fn local_month_day(at: SystemTime, offset_secs: i64) -> (u32, u32, i64) {
    let local = local_secs(at, offset_secs);
    let (_, month, day) = civil_from_days(local.div_euclid(DAY as i64));
    (month, day, local.rem_euclid(DAY as i64))
}

/// Wall clock `HH:MM:SS` at a UTC offset in seconds (`0` for UTC) — for
/// log timestamps.
pub fn clock_hms(at: SystemTime, offset_secs: i64) -> String {
    let day = local_secs(at, offset_secs).rem_euclid(DAY as i64);
    format!(
        "{:02}:{:02}:{:02}",
        day / 3_600,
        (day % 3_600) / 60,
        day % 60
    )
}

/// Wall clock `HH:MM` at a UTC offset in seconds.
pub fn clock_hm(at: SystemTime, offset_secs: i64) -> String {
    let day = local_secs(at, offset_secs).rem_euclid(DAY as i64);
    format!("{:02}:{:02}", day / 3_600, (day % 3_600) / 60)
}

/// A reset time: `14:30` when it lands within a day of `now`, `06-15 09:00`
/// when it is further out (a bare clock would be ambiguous across days).
/// Past instants read as a clock too.
pub fn absolute_label(at: SystemTime, now: SystemTime, offset_secs: i64) -> String {
    let within_day = at
        .duration_since(now)
        .map_or(true, |d| d < Duration::from_secs(DAY));
    if within_day {
        return clock_hm(at, offset_secs);
    }
    let (month, day, _) = local_month_day(at, offset_secs);
    format!("{month:02}-{day:02} {}", clock_hm(at, offset_secs))
}

/// A fixed eleven-cell stamp `MM/DD HH:MM` — wide enough to say which day,
/// narrow enough to sit inside a quota bar.
pub fn absolute_stamp(at: SystemTime, offset_secs: i64) -> String {
    let (month, day, secs) = local_month_day(at, offset_secs);
    format!(
        "{month:02}/{day:02} {:02}:{:02}",
        secs / 3_600,
        (secs % 3_600) / 60
    )
}

/// `M/D HH:MM` with an unpadded month and day, for a banner deadline
/// ("until 7/12 09:00").
pub fn month_day_hm(at: SystemTime, offset_secs: i64) -> String {
    let (month, day, _) = local_month_day(at, offset_secs);
    format!("{month}/{day} {}", clock_hm(at, offset_secs))
}

/// Which local day an instant falls on: whole days since the epoch of the
/// offset-shifted clock. Before 1970 (west of UTC at the epoch) reads day 0.
pub fn local_civil_day(epoch_secs: i64, offset_secs: i64) -> u64 {
    u64::try_from(
        epoch_secs
            .saturating_add(offset_secs)
            .div_euclid(DAY as i64),
    )
    .unwrap_or(0)
}

/// Days since 1970-01-01 → `(year, month, day)` (Howard Hinnant's civil
/// algorithm; no calendar crate needed).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (year + i64::from(month <= 2), month, day)
}

/// The first whitespace-separated word of a display name, clipped to `n`
/// grapheme clusters (`"angelo (U0…)"`, 4 → `ange`). Graphemes, not chars,
/// so a ZWJ emoji or decomposed Hangul stays whole.
pub fn short_name(name: &str, n: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    name.split_whitespace()
        .next()
        .unwrap_or("")
        .graphemes(true)
        .take(n)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    // ---- countdown (m/h/d) ----

    #[test]
    fn countdown_seconds_only() {
        assert_eq!(countdown(secs(0)), "0s");
        assert_eq!(countdown(secs(42)), "42s");
        assert_eq!(countdown(secs(59)), "59s");
    }

    #[test]
    fn countdown_minutes_and_seconds() {
        assert_eq!(countdown(secs(60)), "1m 00s");
        assert_eq!(countdown(secs(5 * 60 + 3)), "5m 03s");
        assert_eq!(countdown(secs(59 * 60 + 59)), "59m 59s");
    }

    #[test]
    fn countdown_hours_and_minutes() {
        assert_eq!(countdown(secs(3_600)), "1h 00m");
        assert_eq!(countdown(secs(2 * 3_600 + 13 * 60)), "2h 13m");
        assert_eq!(countdown(secs(23 * 3_600 + 59 * 60)), "23h 59m");
    }

    #[test]
    fn countdown_days_and_hours() {
        assert_eq!(countdown(secs(86_400)), "1d 0h");
        assert_eq!(countdown(secs(2 * 86_400 + 4 * 3_600)), "2d 4h");
    }

    #[test]
    fn countdown_units_split_matches_countdown_banding() {
        assert_eq!(
            countdown_units(secs(7 * 86_400 + 10 * 3_600)),
            ("7d".to_string(), Some("10h".to_string()))
        );
        assert_eq!(
            countdown_units(secs(10 * 3_600 + 15 * 60)),
            ("10h".to_string(), Some("15m".to_string()))
        );
        assert_eq!(
            countdown_units(secs(17 * 60 + 35)),
            ("17m".to_string(), Some("35s".to_string()))
        );
        assert_eq!(countdown_units(secs(42)), ("42s".to_string(), None));
        assert_eq!(
            countdown_units(secs(3_600 + 5 * 60)),
            ("1h".to_string(), Some("05m".to_string()))
        );
    }

    #[test]
    fn countdown_never_exceeds_its_max_width() {
        for s in [
            0,
            59,
            3_599,
            23 * 3_600 + 59 * 60,
            9 * 86_400 + 23 * 3_600,
            99 * 86_400 + 23 * 3_600,
            100 * 86_400,
        ] {
            assert!(
                countdown(secs(s)).len() <= COUNTDOWN_MAX_WIDTH,
                "{s}: {}",
                countdown(secs(s))
            );
        }
    }

    #[test]
    fn compact_duration_bands() {
        assert_eq!(compact_duration(secs(42)), "42s");
        assert_eq!(compact_duration(secs(192)), "3m12s");
        assert_eq!(compact_duration(secs(6 * 3600 + 52 * 60)), "6h52m");
        assert_eq!(compact_duration(secs(2 * 86_400 + 4 * 3600)), "2d4h");
        assert_eq!(compact_duration(Duration::ZERO), "0s");
    }

    // ---- labels ----

    #[test]
    fn percent_rounds_and_clamps() {
        assert_eq!(percent(0.42), "42%");
        assert_eq!(percent(0.999), "100%");
        assert_eq!(percent(2.0), "100%");
        assert_eq!(percent(-0.3), "0%");
        assert_eq!(percent(f64::NAN), "—", "unknown, not NaN%");
    }

    #[test]
    fn human_count_bands() {
        assert_eq!(human_count(0), "0");
        assert_eq!(human_count(999), "999");
        assert_eq!(human_count(1_200), "1.2k");
        assert_eq!(human_count(999_949), "999.9k");
        assert_eq!(human_count(3_400_000), "3.4M");
    }

    #[test]
    fn human_count_carries_into_the_next_unit() {
        assert_eq!(human_count(999_950), "1.0M");
        assert_eq!(human_count(999_999), "1.0M");
        assert_eq!(human_count(1_000_000), "1.0M");
        assert_eq!(human_count(999_950_000), "1.0G");
    }

    #[test]
    fn human_count_never_clips_a_unit_off_a_big_number() {
        // llmux's models table showed `243578.5` for 243.6 billion tokens: the
        // old ladder stopped at `M` and the column cut the unit off.
        assert_eq!(human_count(243_578_500_000), "243.6G");
        assert_eq!(human_count(7_100_000_000_000), "7.1T");
        assert_eq!(human_count(u64::MAX), "18.4E");
        for n in [1_000u64, 999_949, 999_950, 243_578_500_000, u64::MAX] {
            assert!(crate::text::width(&human_count(n)) <= 6, "{n}");
        }
    }

    #[test]
    fn opt_count_and_tps_are_honest_when_empty() {
        assert_eq!(opt_count(None), "—");
        assert_eq!(opt_count(Some(0)), "0");
        assert_eq!(opt_count(Some(12_345)), "12.3k");
        assert_eq!(tps(None), "—");
        assert_eq!(tps(Some(f64::NAN)), "—");
        assert_eq!(tps(Some(45.4)), "45t/s");
        assert_eq!(tps(Some(7.25)), "7.2t/s");
        assert_eq!(tps(Some(0.0)), "0.0t/s", "a measured zero is a zero");
    }

    // ---- refreshed-ago marker ----

    #[test]
    fn age_unit_is_largest_single_unit() {
        assert_eq!(age(secs(0)), "0s");
        assert_eq!(age(secs(42)), "42s");
        assert_eq!(age(secs(3 * 60 + 12)), "3m");
        assert_eq!(age(secs(6 * 3_600 + 52 * 60)), "6h");
        assert_eq!(age(secs(2 * 86_400 + 3_600)), "2d");
    }

    #[test]
    fn refreshed_marker_renders_age_or_none() {
        let now = SystemTime::UNIX_EPOCH + secs(1_000_000);
        assert_eq!(refreshed_marker(None, now), None, "never refreshed");
        let three_min_ago = now - secs(3 * 60);
        assert_eq!(
            refreshed_marker(Some(three_min_ago), now),
            Some("\u{21bb}3m".into())
        );
        // A future timestamp (clock skew) clamps to zero instead of panicking.
        assert_eq!(
            refreshed_marker(Some(now + secs(5)), now),
            Some("\u{21bb}0s".into())
        );
    }

    #[test]
    fn clock_is_hms_at_an_offset() {
        let at = SystemTime::UNIX_EPOCH + secs(3_661);
        assert_eq!(clock_hms(at, 0), "01:01:01");
        let midnight = SystemTime::UNIX_EPOCH + secs(2 * 86_400);
        assert_eq!(clock_hms(midnight, 0), "00:00:00");
        assert_eq!(clock_hms(midnight, -3_600), "23:00:00", "UTC-1 wraps");
    }

    #[test]
    fn elapsed_one_decimal() {
        assert_eq!(elapsed(Duration::from_millis(3_240)), "3.2s");
    }

    // ---- absolute local time ----

    #[test]
    fn clock_hm_applies_offset() {
        let at = SystemTime::UNIX_EPOCH + secs(12 * 3_600 + 30 * 60); // 12:30 UTC
        assert_eq!(clock_hm(at, 0), "12:30");
        assert_eq!(clock_hm(at, 9 * 3_600), "21:30", "UTC+9");
        assert_eq!(clock_hm(at, -5 * 3_600), "07:30", "UTC-5");
        let late = SystemTime::UNIX_EPOCH + secs(23 * 3_600);
        assert_eq!(clock_hm(late, 2 * 3_600), "01:00");
    }

    #[test]
    fn absolute_label_within_a_day_is_clock_only() {
        let now = SystemTime::UNIX_EPOCH + secs(1_000_000);
        let soon = now + secs(3_720);
        assert_eq!(absolute_label(soon, now, 0), clock_hm(soon, 0));
        // Past timestamps degrade to clock form too (no negative dates).
        assert_eq!(absolute_label(now, now, 0), clock_hm(now, 0));
    }

    #[test]
    fn absolute_label_beyond_a_day_includes_the_date() {
        // 2026-06-13 00:00:00 UTC = 1781308800.
        let now = SystemTime::UNIX_EPOCH + secs(1_781_308_800);
        let in_two_days = now + secs(2 * 86_400 + 9 * 3_600);
        assert_eq!(absolute_label(in_two_days, now, 0), "06-15 09:00");
        // UTC+9 pushes it past the next local midnight.
        let in_30h = now + secs(30 * 3_600);
        assert_eq!(absolute_label(in_30h, now, 9 * 3_600), "06-14 15:00");
    }

    #[test]
    fn absolute_stamp_is_eleven_cells_and_uses_the_shared_civil_date() {
        // 2026-06-13 00:00:00 UTC.
        let at = SystemTime::UNIX_EPOCH + secs(1_781_308_800 + 9 * 3_600 + 5 * 60);
        assert_eq!(absolute_stamp(at, 0), "06/13 09:05");
        assert_eq!(absolute_stamp(at, 15 * 3_600), "06/14 00:05", "UTC+15");
        assert_eq!(absolute_stamp(at, -10 * 3_600), "06/12 23:05", "UTC-10");
        // Leap day, and the epoch itself.
        let leap = SystemTime::UNIX_EPOCH + secs(19_782 * 86_400 + 12 * 3_600);
        assert_eq!(absolute_stamp(leap, 0), "02/29 12:00");
        assert_eq!(absolute_stamp(SystemTime::UNIX_EPOCH, 0), "01/01 00:00");
        for s in [0, 1_781_308_800, 4_102_444_800] {
            let at = SystemTime::UNIX_EPOCH + secs(s);
            assert_eq!(absolute_stamp(at, 0).len(), 11);
        }
    }

    #[test]
    fn month_day_hm_is_unpadded_date_padded_clock() {
        // 2026-07-12 00:00 UTC = 1783814400.
        let at = SystemTime::UNIX_EPOCH + secs(1_783_814_400);
        assert_eq!(month_day_hm(at, 0), "7/12 00:00");
        assert_eq!(month_day_hm(at, 9 * 3_600), "7/12 09:00");
        assert_eq!(month_day_hm(at, -3_600), "7/11 23:00");
    }

    #[test]
    fn local_civil_day_shifts_and_clamps() {
        assert_eq!(local_civil_day(0, 0), 0);
        assert_eq!(local_civil_day(86_399, 0), 0);
        assert_eq!(local_civil_day(86_399, 1), 1, "the offset crosses midnight");
        assert_eq!(local_civil_day(0, -3_600), 0, "pre-epoch clamps to day 0");
    }

    #[test]
    fn civil_from_days_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        // Leap day.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    // ---- remaining-time formatter (event banner) ----

    #[test]
    fn remaining_hms_multi_day_shows_all_units() {
        let d = secs(3 * 86_400 + 15 * 3_600 + 15 * 60 + 25);
        assert_eq!(remaining_hms(d), "3d 15h 15m 25s");
        // Zero lower units still render once a higher unit is present.
        assert_eq!(remaining_hms(secs(2 * 86_400)), "2d 0h 0m 0s");
    }

    #[test]
    fn remaining_hms_under_an_hour_omits_leading_zero_units() {
        assert_eq!(remaining_hms(secs(15 * 60 + 25)), "15m 25s");
        assert_eq!(remaining_hms(secs(5 * 60)), "5m 0s");
        assert_eq!(remaining_hms(secs(3_600 + 2)), "1h 0m 2s");
    }

    #[test]
    fn remaining_hms_under_a_minute_is_seconds_only() {
        assert_eq!(remaining_hms(secs(25)), "25s");
        assert_eq!(remaining_hms(secs(59)), "59s");
    }

    #[test]
    fn remaining_hms_zero_is_zero_seconds() {
        assert_eq!(remaining_hms(secs(0)), "0s");
    }

    #[test]
    fn remaining_hms_bound_covers_every_value_with_the_same_lead() {
        for lead in [
            secs(30),
            secs(15 * 60),
            secs(5 * 3_600),
            secs(3 * 86_400),
            secs(12 * 86_400),
        ] {
            let bound = remaining_hms_bound(lead);
            let (d, h, m, _) = units(lead);
            // Every second within the same leading unit stays inside the bound.
            let step = if d > 0 {
                DAY
            } else if h > 0 {
                HOUR
            } else if m > 0 {
                MINUTE
            } else {
                1
            };
            let base = lead.as_secs() / step * step;
            for s in (base..base + step).step_by(7) {
                assert!(remaining_hms(secs(s)).len() <= bound, "{s}");
            }
        }
    }

    // ---- display-name shortening ----

    #[test]
    fn short_name_first_token_first_four_graphemes() {
        assert_eq!(short_name("Z (U09F1M5MML1)", 4), "Z");
        assert_eq!(short_name("luka (U0AAAAAAAAA)", 4), "luka");
        assert_eq!(short_name("angelo (U0BBBBBBBBB)", 4), "ange");
        assert_eq!(short_name("local", 4), "loca");
    }

    #[test]
    fn short_name_multibyte_never_splits_a_char() {
        assert_eq!(short_name("위대한이름 (U1)", 4), "위대한이");
        assert_eq!(short_name("한글", 4), "한글");
    }

    #[test]
    fn short_name_counts_grapheme_clusters() {
        // A ZWJ family emoji is one grapheme (seven scalars).
        assert_eq!(short_name("👨‍👩‍👧‍👦 (U1)", 4), "👨‍👩‍👧‍👦");
        assert_eq!(short_name("👨‍👩‍👧‍👦x둘셋넷 (U1)", 4), "👨‍👩‍👧‍👦x둘셋");
        // Decomposed Hangul (conjoining jamo): six scalars, two graphemes.
        assert_eq!(
            short_name("\u{1112}\u{1161}\u{11AB}\u{1100}\u{1173}\u{11AF}", 4),
            "\u{1112}\u{1161}\u{11AB}\u{1100}\u{1173}\u{11AF}"
        );
    }

    #[test]
    fn short_name_empty_and_whitespace_are_empty() {
        assert_eq!(short_name("", 4), "");
        assert_eq!(short_name("   ", 4), "");
    }

    // ---- money ----

    #[test]
    fn cost_decimal_scheme() {
        assert_eq!(cost(0.0), "$0.0000");
        assert_eq!(cost(0.0123), "$0.0123");
        assert_eq!(cost(0.999_94), "$0.9999");
        assert_eq!(cost(1.0), "$1.00");
        assert_eq!(cost(3.775), "$3.77");
        assert_eq!(cost(12.5), "$12.50");
    }

    #[test]
    fn cost_carries_and_is_honest_about_invalid_amounts() {
        assert_eq!(cost(0.999_96), "$1.00", "no $1.0000");
        assert_eq!(cost(-0.42), "—");
        assert_eq!(cost(f64::NAN), "—");
        assert_eq!(cost(f64::INFINITY), "—");
    }

    #[test]
    fn cost_parts_carry_grouping_and_subdollar_precision() {
        assert_eq!(cost_parts(0.99999), ("1".into(), ".00".into()));
        assert_eq!(cost_parts(0.0076), ("0".into(), ".0076".into()));
        assert_eq!(cost_parts(0.0), ("0".into(), ".0000".into()));
        assert_eq!(cost_parts(1.25), ("1".into(), ".25".into()));
        assert_eq!(cost_parts(1_234.499), ("1,234".into(), ".50".into()));
        assert_eq!(
            cost_parts(1_234_567.891),
            ("1,234,567".into(), ".89".into())
        );
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1_000), "1,000");
        assert_eq!(group_thousands(12_345_678), "12,345,678");
    }
}
