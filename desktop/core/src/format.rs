//! Short human-readable times and amounts, matching the widget and tray icon.

use chrono::{Local, TimeZone};

/// "1d 4h", "2h 31m" or "5m" (never less than a minute).
pub fn duration(secs: f64) -> String {
    let secs = secs.max(0.0) as i64;
    let (d, h, m) = (secs / 86400, secs % 86400 / 3600, secs % 3600 / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{}m", m.max(1))
    }
}

/// "$12.34", or "$123" from 100 up. Other currencies get their code.
pub fn money(value: Option<f64>, currency: &str) -> String {
    let Some(value) = value else {
        return "–".into();
    };
    let symbol = if currency.is_empty() || currency == "USD" { "$".to_string() } else { format!("{currency} ") };
    let digits = if value.abs() >= 100.0 { 0 } else { 2 };
    format!("{symbol}{}", group_thousands(&format!("{value:.digits$}")))
}

fn group_thousands(number: &str) -> String {
    let (int, frac) = number.split_once('.').map_or((number, None), |(i, f)| (i, Some(f)));
    let (sign, int) = int.strip_prefix('-').map_or(("", int), |i| ("-", i));
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    match frac {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    }
}

/// Local reset time: "14:37" today, "Thu 14:37" otherwise.
pub fn reset_time(epoch: f64, now: f64) -> String {
    let (Some(when), Some(today)) =
        (Local.timestamp_opt(epoch as i64, 0).single(), Local.timestamp_opt(now as i64, 0).single())
    else {
        return String::new();
    };
    if when.date_naive() == today.date_naive() {
        when.format("%H:%M").to_string()
    } else {
        when.format("%a %H:%M").to_string()
    }
}

/// "just now", "3 minutes ago", or "at 14:37" after an hour.
pub fn ago(epoch: f64, now: f64) -> String {
    let mins = ((now - epoch) / 60.0).floor() as i64;
    match mins {
        m if m < 1 => "just now".into(),
        1 => "1 minute ago".into(),
        m if m < 60 => format!("{m} minutes ago"),
        _ => Local
            .timestamp_opt(epoch as i64, 0)
            .single()
            .map(|t| format!("at {}", t.format("%H:%M")))
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(duration(30.0), "1m");
        assert_eq!(duration(2533.0), "42m");
        assert_eq!(duration(3.0 * 3600.0 + 7.0 * 60.0), "3h 7m");
        assert_eq!(duration(2.0 * 86400.0 + 4.0 * 3600.0 + 59.0), "2d 4h");
    }

    #[test]
    fn amounts() {
        assert_eq!(money(Some(12.345), "USD"), "$12.35");
        assert_eq!(money(Some(42.75), ""), "$42.75");
        assert_eq!(money(Some(1234.4), "USD"), "$1,234");
        assert_eq!(money(Some(5.0), "EUR"), "EUR 5.00");
        assert_eq!(money(None, "USD"), "–");
    }

    #[test]
    fn ages() {
        assert_eq!(ago(1000.0, 1030.0), "just now");
        assert_eq!(ago(1000.0, 1090.0), "1 minute ago");
        assert_eq!(ago(1000.0, 1000.0 + 25.0 * 60.0), "25 minutes ago");
        assert!(ago(1000.0, 1000.0 + 2.0 * 3600.0).starts_with("at "));
    }
}
