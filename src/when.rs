//! Display times. Provider timestamps are UTC Unix seconds and are shown in local time; demo
//! fixtures carry ordinals instead of dates, so their prepared label is used unchanged.
use chrono::{DateTime, Datelike, Local, TimeZone};
use std::fmt::Display;

/// Smaller values are fixture ordinals, not dates (2001-09-09 UTC).
const FLOOR: u64 = 1_000_000_000;

fn at<Tz: TimeZone>(timestamp: u64, zone: &Tz) -> Option<DateTime<Tz>> {
    (timestamp >= FLOOR)
        .then(|| zone.timestamp_opt(timestamp as i64, 0).single())
        .flatten()
}

/// Compact feed time: "14:03" today, "18 Oct 14:03" this year, "18 Oct 2025" before.
pub fn short(timestamp: u64, fallback: &str) -> String {
    short_at(timestamp, fallback, &Local::now())
}

fn short_at<Tz: TimeZone>(timestamp: u64, fallback: &str, now: &DateTime<Tz>) -> String
where
    Tz::Offset: Display,
{
    match at(timestamp, &now.timezone()) {
        None => fallback.to_owned(),
        Some(t) if t.date_naive() == now.date_naive() => t.format("%H:%M").to_string(),
        Some(t) if t.year() == now.year() => t.format("%-d %b %H:%M").to_string(),
        Some(t) => t.format("%-d %b %Y").to_string(),
    }
}

/// Time of day only, e.g. "14:03".
pub fn clock(timestamp: u64, fallback: &str) -> String {
    at(timestamp, &Local).map_or_else(|| fallback.to_owned(), |t| t.format("%H:%M").to_string())
}

/// Calendar day, e.g. "Sun 18 Oct 2026"; fixtures have none.
pub fn day(timestamp: u64) -> Option<String> {
    at(timestamp, &Local).map(|t| t.format("%a %-d %b %Y").to_string())
}

/// Full local date and time with its UTC offset, e.g. "Sun 18 Oct 2026, 14:03 (UTC+11:00)".
pub fn full(timestamp: u64, fallback: &str) -> String {
    full_in(timestamp, fallback, &Local)
}

fn full_in<Tz: TimeZone>(timestamp: u64, fallback: &str, zone: &Tz) -> String
where
    Tz::Offset: Display,
{
    at(timestamp, zone).map_or_else(
        || fallback.to_owned(),
        |t| t.format("%a %-d %b %Y, %H:%M (UTC%:z)").to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    #[test]
    fn times_are_local_compact_in_the_feed_and_complete_on_request() {
        let zone = FixedOffset::east_opt(11 * 3600).unwrap();
        // 2026-10-18 03:03 UTC is 14:03 the same day at UTC+11.
        let timestamp = 1_792_292_580;
        let now = zone.timestamp_opt(timestamp as i64 + 3600, 0).unwrap();
        assert_eq!(short_at(timestamp, "x", &now), "14:03");
        let next_week = zone
            .timestamp_opt(timestamp as i64 + 7 * 86_400, 0)
            .unwrap();
        assert_eq!(short_at(timestamp, "x", &next_week), "18 Oct 14:03");
        let next_year = zone
            .timestamp_opt(timestamp as i64 + 400 * 86_400, 0)
            .unwrap();
        assert_eq!(short_at(timestamp, "x", &next_year), "18 Oct 2026");
        assert_eq!(
            full_in(timestamp, "x", &zone),
            "Sun 18 Oct 2026, 14:03 (UTC+11:00)"
        );
        // Fixture ordinals keep their prepared label rather than becoming 1970 dates.
        assert_eq!(short_at(1_005, "10:40", &now), "10:40");
        assert_eq!(full_in(1_005, "10:40", &zone), "10:40");
        assert_eq!(day(1_005), None);
    }
}
