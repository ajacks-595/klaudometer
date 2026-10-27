//! Turning fetched usage into meters (with colours and pace), the lines the
//! tray menu shows, and the alerts to send. Mirrors the widget's main.qml and
//! the Linux tray icon, including alert IDs, so every front end shares one
//! alert record.

use crate::format;
use crate::model::Usage;
use crate::settings::Settings;
use chrono::{TimeZone, Utc};
use serde::Serialize;

/// Ring colours: one per meter kind, plus the warning and critical colours
/// rings switch to as usage rises.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Colors {
    pub five_hour: String,
    pub seven_day: String,
    pub spend: String,
    pub warning: String,
    pub critical: String,
}

/// The theme's colours, with any single-colour overrides from the settings.
pub fn palette(settings: &Settings) -> Colors {
    let [five_hour, seven_day, spend, warning, critical] = match settings.theme.as_str() {
        "ocean" => ["#38BDF8", "#2DD4BF", "#A78BFA", "#FBBF24", "#F43F5E"],
        "sunset" => ["#FB923C", "#C084FC", "#F472B6", "#FACC15", "#EF4444"],
        "forest" => ["#A3C55B", "#3FAE8C", "#5FA8D3", "#E9B949", "#D9534F"],
        // Mid greys, so they read on light and dark panels alike.
        "mono" => ["#A8A8A8", "#787878", "#909090", "#E8A33D", "#E5484D"],
        _ => ["#D97757", "#6A9BCC", "#8DB06E", "#E8A33D", "#E5484D"],
    };
    let pick = |override_: &str, default: &str| {
        if override_.starts_with('#') { override_.to_string() } else { default.to_string() }
    };
    Colors {
        five_hour: pick(&settings.five_hour_color, five_hour),
        seven_day: pick(&settings.seven_day_color, seven_day),
        spend: pick(&settings.spend_color, spend),
        warning: pick(&settings.warning_color, warning),
        critical: pick(&settings.critical_color, critical),
    }
}

/// One meter's live state. `seconds_left` and `elapsed` are -1 when unknown.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Meter {
    pub id: String,
    pub name: String,
    pub short: String,
    /// Money meters (extra usage, API spend) count currency, not a window.
    pub money: bool,
    /// Ring segments: one per hour for 5h, per day for 7d, none for money.
    pub segments: u32,
    pub base_color: String,
    pub color: String,
    pub has_data: bool,
    pub stale: bool,
    /// Percent of the window, budget or cap.
    pub used: f64,
    pub left: f64,
    pub shown: f64,
    pub spent: Option<f64>,
    pub limit: Option<f64>,
    pub today: Option<f64>,
    pub currency: String,
    pub resets_at: f64,
    pub seconds_left: f64,
    pub length: f64,
    pub elapsed: f64,
    /// The window rolled over since the last fetch.
    pub reset: bool,
    pub pace_known: bool,
    /// Percent used by the reset at the current rate.
    pub projected: Option<f64>,
    pub seconds_to_limit: Option<f64>,
    /// Hits 100% before it resets.
    pub runs_out: bool,
}

/// The meters to show, in order. Extra usage only appears while it's on.
pub fn meter_ids(settings: &Settings, usage: Option<&Usage>) -> Vec<&'static str> {
    let mut ids = Vec::new();
    if settings.show_five_hour {
        ids.push("five_hour");
    }
    if settings.show_seven_day {
        ids.push("seven_day");
    }
    if settings.show_extra && usage.is_some_and(|u| u.subscription.extra.is_some()) {
        ids.push("extra");
    }
    if settings.show_api {
        ids.push("api");
    }
    if ids.is_empty() {
        ids = vec!["five_hour", "seven_day"];
    }
    ids
}

pub fn build_meters(usage: Option<&Usage>, settings: &Settings, now: f64) -> Vec<Meter> {
    let colors = palette(settings);
    meter_ids(settings, usage).into_iter().map(|id| build_meter(id, usage, settings, &colors, now)).collect()
}

fn build_meter(id: &str, usage: Option<&Usage>, settings: &Settings, colors: &Colors, now: f64) -> Meter {
    let (name, short, base, segments) = match id {
        "five_hour" => ("5-hour", "5h", &colors.five_hour, 5),
        "seven_day" => ("7-day", "7d", &colors.seven_day, 7),
        "extra" => ("Extra usage", "Extra", &colors.spend, 1),
        _ => ("API spend", "API", &colors.spend, 1),
    };
    let mut m = Meter {
        id: id.into(),
        name: name.into(),
        short: short.into(),
        money: matches!(id, "extra" | "api"),
        segments,
        base_color: base.clone(),
        color: base.clone(),
        has_data: false,
        stale: usage.is_some_and(|u| u.subscription.stale),
        used: 0.0,
        left: 100.0,
        shown: 0.0,
        spent: None,
        limit: None,
        today: None,
        currency: "USD".into(),
        resets_at: 0.0,
        seconds_left: -1.0,
        length: 0.0,
        elapsed: -1.0,
        reset: false,
        pace_known: false,
        projected: None,
        seconds_to_limit: None,
        runs_out: false,
    };
    if let Some(usage) = usage {
        if m.money {
            fill_money(&mut m, usage, settings, now);
        } else {
            fill_window(&mut m, usage, now);
        }
    }
    m.left = (100.0 - m.used).max(0.0);
    m.shown = if settings.show_remaining { m.left } else { m.used };
    fill_pace(&mut m);
    if m.has_data && settings.color_by_level {
        if m.used >= settings.critical {
            m.color = colors.critical.clone();
        } else if m.used >= settings.warning {
            m.color = colors.warning.clone();
        }
    }
    m
}

fn fill_window(m: &mut Meter, usage: &Usage, now: f64) {
    let Some(w) = usage.subscription.windows.get(&m.id) else { return };
    m.has_data = true;
    m.resets_at = w.resets_at.unwrap_or(0.0);
    m.seconds_left = if m.resets_at > 0.0 { (m.resets_at - now).max(0.0) } else { -1.0 };
    m.reset = m.seconds_left == 0.0;
    m.used = if m.reset { 0.0 } else { w.used.clamp(0.0, 100.0) };
    m.length = w.length;
    if m.seconds_left > 0.0 && w.length > 0.0 {
        m.elapsed = (1.0 - m.seconds_left / w.length).min(1.0);
    }
}

fn fill_money(m: &mut Meter, usage: &Usage, settings: &Settings, now: f64) {
    let mut percent = None;
    if m.id == "api" {
        let Some(a) = usage.api.as_ref().filter(|a| a.spent.is_some()) else { return };
        m.stale = a.stale;
        m.spent = a.spent;
        m.today = a.today;
        if !a.currency.is_empty() {
            m.currency = a.currency.clone();
        }
        m.limit = (settings.api_budget > 0.0).then_some(settings.api_budget);
        m.resets_at = a.next_reset;
        m.seconds_left = if a.next_reset > 0.0 { (a.next_reset - now).max(0.0) } else { -1.0 };
        if m.seconds_left > 0.0 && a.month_start > 0.0 {
            m.length = a.next_reset - a.month_start;
            m.elapsed = (1.0 - m.seconds_left / m.length).min(1.0);
        }
    } else {
        let Some(x) = &usage.subscription.extra else { return };
        m.spent = x.used;
        m.limit = x.limit;
        if !x.currency.is_empty() {
            m.currency = x.currency.clone();
        }
        percent = x.percent;
    }
    if percent.is_none() {
        if let (Some(spent), Some(limit)) = (m.spent, m.limit) {
            percent = Some(spent / limit * 100.0);
        }
    }
    m.has_data = true;
    // Spend can run past the budget; the ring just stays full.
    m.used = percent.unwrap_or(0.0).max(0.0);
}

/// Straight-line projection from the average rate since the window (or
/// month) began. Early on that's mostly noise, so it waits until a tenth of
/// the time has passed.
fn fill_pace(m: &mut Meter) {
    if !m.has_data || m.reset || m.elapsed < 0.1 || m.length <= 0.0 || (m.money && m.limit.is_none()) {
        return;
    }
    m.pace_known = true;
    m.projected = Some(m.used / m.elapsed);
    if m.used >= 100.0 {
        m.seconds_to_limit = Some(0.0);
    } else if m.used > 0.0 {
        m.seconds_to_limit = Some((100.0 - m.used) * m.elapsed * m.length / m.used);
    }
    m.runs_out = m.seconds_to_limit.is_some_and(|s| s < m.seconds_left);
}

/// "Runs out in ~42m", "On pace for ~82%" and so on; empty when unknown.
pub fn pace_text(m: &Meter, settings: &Settings) -> String {
    if !settings.show_pace || !m.pace_known || m.used >= 100.0 {
        return String::new();
    }
    if m.runs_out {
        let when = format::duration(m.seconds_to_limit.unwrap_or(0.0));
        return if m.money { format!("Over budget in ~{when}") } else { format!("Runs out in ~{when}") };
    }
    let projected = m.projected.unwrap_or(0.0);
    if m.money {
        format!("On pace for ~{}", format::money(m.spent.map(|s| s / m.elapsed), &m.currency))
    } else if settings.show_remaining {
        format!("~{}% left at reset", (100.0 - projected).round())
    } else {
        format!("On pace for ~{}%", projected.round())
    }
}

/// A meter's headline and detail line, as the tray menu shows them.
pub fn meter_lines(m: &Meter, settings: &Settings, now: f64, ring: &str) -> (String, String) {
    let name = if ring.is_empty() { m.name.clone() } else { format!("{} ({ring} ring)", m.name) };
    if !m.has_data {
        return (format!("{name}: no data"), String::new());
    }
    if m.reset {
        return (format!("{name}: just reset"), String::new());
    }
    let mut details = Vec::new();
    let head = if m.money {
        if m.seconds_left > 0.0 {
            details.push(format!("Resets in {}", format::duration(m.seconds_left)));
        }
        let spent = format::money(m.spent, &m.currency);
        match m.limit {
            Some(_) => format!("{name}: {spent} of {} ({}%)", format::money(m.limit, &m.currency), m.used.round()),
            None => format!("{name}: {spent}"),
        }
    } else {
        if m.seconds_left > 0.0 {
            details.push(format!(
                "Resets in {} ({})",
                format::duration(m.seconds_left),
                format::reset_time(m.resets_at, now)
            ));
        }
        format!("{name}: {}% used, {}% left", m.used.round(), m.left.round())
    };
    let pace = pace_text(m, settings);
    if !pace.is_empty() {
        details.push(pace);
    }
    (head, details.join(" · "))
}

/// Text beside the tray icon: "32% · 70%" or "5h 32% · 7d 70%".
pub fn label_text(meters: &[Meter], settings: &Settings) -> String {
    if settings.label != "full" && settings.label != "compact" {
        return String::new();
    }
    meters
        .iter()
        .map(|m| {
            let value = if !m.has_data {
                "–".to_string()
            } else if m.money && m.limit.is_none() {
                format::money(m.spent, &m.currency)
            } else {
                format!("{}%", m.shown.round())
            };
            if settings.label == "full" { format!("{} {value}", m.short) } else { value }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// One line of the tray menu. Detail lines are shown greyed out.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    pub key: String,
    pub text: String,
    pub detail: bool,
}

/// One meter as the card shows it, with its text already formatted (the
/// same wording as the widget's desktop card).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Card {
    pub id: String,
    pub title: String,
    pub color: String,
    pub base_color: String,
    pub segments: u32,
    /// Filled share of the ring, 0..1.
    pub fraction: f64,
    /// Where the time-passed tick goes, 0..1, or -1 for none.
    pub marker: f64,
    /// The number in the ring ("42%", "$42.75") and the line under it.
    pub big: String,
    pub sub: String,
    /// "58% left", "$12.50 today"...
    pub caption: String,
    pub resets: String,
    pub pace: String,
    pub runs_out: bool,
    pub stale: bool,
}

pub fn card(m: &Meter, settings: &Settings) -> Card {
    let remaining = settings.show_remaining;
    let money = |v: Option<f64>| format::money(v, &m.currency);
    let big = if !m.has_data {
        "–".to_string()
    } else if !m.money {
        format!("{}%", m.shown.round())
    } else if remaining && m.limit.is_some() {
        money(m.limit.zip(m.spent).map(|(limit, spent)| (limit - spent).max(0.0)))
    } else {
        money(m.spent)
    };
    let sub = match (m.money, m.limit) {
        (false, _) if remaining => "left".to_string(),
        (false, _) => "used".to_string(),
        (true, None) => "this month".to_string(),
        (true, Some(_)) if remaining => format!("left of {}", money(m.limit)),
        (true, Some(_)) => format!("of {}", money(m.limit)),
    };
    let caption = if !m.has_data {
        String::new()
    } else if m.money && m.limit.is_none() {
        m.today.map(|t| format!("{} today", money(Some(t)))).unwrap_or_default()
    } else if remaining {
        format!("{}% used", m.used.round())
    } else {
        format!("{}% left", m.left.round())
    };
    let resets = if !m.has_data {
        String::new()
    } else if m.reset {
        "Just reset".to_string()
    } else if m.seconds_left > 0.0 {
        format!("Resets in {}", format::duration(m.seconds_left))
    } else {
        String::new()
    };
    let fraction = if !m.has_data || (m.money && m.limit.is_none()) { 0.0 } else { (m.shown / 100.0).clamp(0.0, 1.0) };
    let marker = match m.elapsed {
        e if e < 0.0 => -1.0,
        e if remaining => 1.0 - e,
        e => e,
    };
    Card {
        id: m.id.clone(),
        title: if m.money { m.name.clone() } else { format!("{} window", m.name) },
        color: m.color.clone(),
        base_color: m.base_color.clone(),
        segments: m.segments,
        fraction,
        marker,
        big,
        sub,
        caption,
        resets,
        pace: pace_text(m, settings),
        runs_out: m.runs_out && m.used < 100.0,
        stale: m.stale,
    }
}

/// Everything a front end shows for one moment.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Summary {
    pub meters: Vec<Meter>,
    pub cards: Vec<Card>,
    /// The first two meters: inner and outer ring of the tray icon.
    pub rings: Vec<Meter>,
    pub label: String,
    pub rows: Vec<Row>,
    pub plan: String,
    pub errors: Vec<String>,
    /// The card's last line: errors, "Updated 3 minutes ago" or "Loading…".
    pub footer: String,
}

pub fn summary(usage: Option<&Usage>, settings: &Settings, now: f64) -> Summary {
    let meters = build_meters(usage, settings, now);
    let rings: Vec<Meter> = meters.iter().take(2).cloned().collect();
    let ring_name = |id: &str| match rings.iter().position(|r| r.id == id) {
        Some(0) if rings.len() == 2 => "inner",
        Some(1) => "outer",
        _ => "",
    };
    let plan = usage.map(|u| u.subscription.plan.clone()).unwrap_or_default();
    let row = |key: &str, text: String, detail: bool| Row { key: key.into(), text, detail };
    let mut rows = vec![row(
        "title",
        if plan.is_empty() { "Klaudometer".into() } else { format!("Klaudometer · {plan}") },
        true,
    )];
    for m in &meters {
        let (head, detail) = meter_lines(m, settings, now, ring_name(&m.id));
        rows.push(row(&m.id, head, false));
        if !detail.is_empty() {
            rows.push(row(&format!("{}:detail", m.id), format!("    {detail}"), true));
        }
    }
    let mut errors = Vec::new();
    if let Some(usage) = usage {
        if let Some(e) = usage.subscription.error.as_ref().filter(|_| settings.wants_subscription()) {
            errors.push(e.clone());
        }
        if let Some(e) = usage.api.as_ref().and_then(|a| a.error.as_ref()).filter(|_| settings.show_api) {
            errors.push(format!("API spend: {e}"));
        }
    }
    let fetched_at = usage
        .map(|u| u.subscription.fetched_at.max(u.api.as_ref().map_or(0.0, |a| a.fetched_at)))
        .filter(|t| *t > 0.0);
    let footer = if !errors.is_empty() {
        rows.extend(errors.iter().enumerate().map(|(i, e)| row(&format!("error{i}"), e.clone(), true)));
        errors.join("\n")
    } else {
        let text = match fetched_at {
            Some(t) => format!("Updated {}", format::ago(t, now)),
            None => "Loading…".into(),
        };
        rows.push(row("updated", text.clone(), true));
        text
    };
    let label = label_text(&rings, settings);
    let cards = meters.iter().map(|m| card(m, settings)).collect();
    Summary { meters, cards, rings, label, rows, plan, errors, footer }
}

/// A notification to send if no other front end has sent it already.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Alert {
    pub id: String,
    pub title: String,
    pub body: String,
    /// "low", "normal" or "high".
    pub level: String,
}

/// Alerts due for these meters, with the same IDs the widget and tray use.
pub fn alert_requests(meters: &[Meter], settings: &Settings, usage: &Usage, now: f64) -> Vec<Alert> {
    let month = Utc.timestamp_opt(now as i64, 0).single().map(|t| t.format("%Y-%m").to_string()).unwrap_or_default();
    let mut alerts = Vec::new();
    for m in meters {
        let unusable = if m.money { m.limit.is_none() } else { m.resets_at <= 0.0 };
        if !m.has_data || m.stale || unusable {
            continue;
        }
        let mut kinds = Vec::new();
        if m.reset {
            let had_usage = usage.subscription.windows.get(&m.id).is_some_and(|w| w.used > 0.0);
            if settings.notify_reset && had_usage && now - m.resets_at < 600.0 {
                kinds.push("reset".to_string());
            }
        } else {
            if settings.notify_critical && m.used >= settings.critical {
                kinds.push(format!("critical@{}", settings.critical));
            } else if settings.notify_warning && m.used >= settings.warning {
                kinds.push(format!("warning@{}", settings.warning));
            }
            if settings.notify_pace && m.runs_out && m.used < 100.0 {
                kinds.push("pace".to_string());
            }
        }
        // Money resets monthly. Window reset times jitter by fractions of a
        // second between fetches, so they're rounded to the minute.
        let period = if m.money { month.clone() } else { format!("{}", (m.resets_at / 60.0).round() as i64) };
        for kind in kinds {
            let (title, body, level) = alert_message(m, &kind);
            alerts.push(Alert { id: format!("{}:{kind}:{period}", m.id), title, body, level: level.into() });
        }
    }
    alerts
}

fn alert_message(m: &Meter, kind: &str) -> (String, String, &'static str) {
    let money = |v: Option<f64>| format::money(v, &m.currency);
    if kind == "reset" {
        return (
            format!("Claude {} limit has reset", m.name),
            format!("Your full {} allowance is available again.", m.name),
            "low",
        );
    }
    if kind == "pace" {
        let to_limit = m.seconds_to_limit.unwrap_or(0.0);
        return if m.money {
            (
                "Claude API spend on pace to pass your budget".into(),
                format!(
                    "At this rate you'll spend ~{} this month (budget {}).",
                    money(m.spent.map(|s| s / m.elapsed)),
                    money(m.limit)
                ),
                "normal",
            )
        } else {
            (
                format!("Claude {} limit: on pace to run out", m.name),
                format!(
                    "At this rate you'll hit it in ~{}, {} before it resets.",
                    format::duration(to_limit),
                    format::duration(m.seconds_left - to_limit)
                ),
                "normal",
            )
        };
    }
    let level = if kind.starts_with("critical") { "high" } else { "normal" };
    if m.money {
        let (spent, limit) = (money(m.spent), money(m.limit));
        return if m.id == "api" {
            (
                format!("Claude API spend at {}% of budget", m.used.round()),
                format!("{spent} of your {limit} monthly budget spent."),
                level,
            )
        } else {
            (
                format!("Claude extra usage at {}% of your limit", m.used.round()),
                format!("{spent} of {limit} used this month."),
                level,
            )
        };
    }
    let mut body = if level == "high" {
        format!("Only {}% left.", m.left.round())
    } else {
        format!("{}% left.", m.left.round())
    };
    if m.seconds_left > 0.0 {
        body += &format!(" Resets in {}.", format::duration(m.seconds_left));
    }
    (format!("Claude {} usage at {}%", m.name, m.used.round()), body, level)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ApiSpend, Extra, Subscription, Window};

    const NOW: f64 = 1_793_085_000.0;

    /// The mock data the other front ends were tested with: 5h at 81% with 2h
    /// left, 7d at 47% with 3 days left, extra usage $12.34 of $50, and API
    /// spend $42.75 this month.
    fn mock_usage() -> Usage {
        let mut windows = std::collections::BTreeMap::new();
        windows.insert("five_hour".into(), Window { used: 81.0, resets_at: Some(NOW + 7200.0), length: 18000.0 });
        windows.insert("seven_day".into(), Window { used: 47.0, resets_at: Some(NOW + 3.0 * 86400.0), length: 604800.0 });
        Usage {
            subscription: Subscription {
                fetched_at: NOW,
                plan: "Max 5x".into(),
                windows,
                extra: Some(Extra { used: Some(12.34), limit: Some(50.0), percent: Some(24.68), currency: "USD".into() }),
                ..Default::default()
            },
            api: Some(ApiSpend {
                spent: Some(42.75),
                today: Some(12.5),
                currency: "USD".into(),
                month_start: NOW - 26.4 * 86400.0,
                next_reset: NOW + 4.6 * 86400.0,
                fetched_at: NOW,
                ..Default::default()
            }),
        }
    }

    fn settings_with_api() -> Settings {
        Settings { show_api: true, notify_pace: true, ..Settings::default() }
    }

    #[test]
    fn windows_pace_and_colours() {
        let meters = build_meters(Some(&mock_usage()), &settings_with_api(), NOW);
        let ids: Vec<_> = meters.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["five_hour", "seven_day", "extra", "api"]);

        let five = &meters[0];
        assert_eq!((five.used, five.left, five.segments), (81.0, 19.0, 5));
        assert!((five.elapsed - 0.6).abs() < 1e-9);
        assert!(five.runs_out);
        assert!((five.seconds_to_limit.unwrap() - 2533.33).abs() < 0.01);
        assert_eq!(five.color, "#E8A33D", "81% is past the 75% warning level");

        let seven = &meters[1];
        assert!(!seven.runs_out);
        assert_eq!(seven.color, "#6A9BCC");
        assert_eq!(pace_text(seven, &settings_with_api()), "On pace for ~82%");
        assert_eq!(pace_text(five, &settings_with_api()), "Runs out in ~42m");
    }

    #[test]
    fn money_meters() {
        let meters = build_meters(Some(&mock_usage()), &settings_with_api(), NOW);
        let (extra, api) = (&meters[2], &meters[3]);
        assert_eq!((extra.spent, extra.limit, extra.used, extra.segments), (Some(12.34), Some(50.0), 24.68, 1));
        assert!(!extra.pace_known, "extra usage has no reset time");
        assert!((api.used - 42.75).abs() < 1e-9, "a $100 budget makes dollars equal percent");
        // $42.75 after 26.4 of 31 days.
        assert_eq!(pace_text(api, &settings_with_api()), "On pace for ~$50.20");
    }

    #[test]
    fn rows_and_label() {
        let s = summary(Some(&mock_usage()), &Settings { label: "full".into(), ..settings_with_api() }, NOW);
        let texts: Vec<_> = s.rows.iter().map(|r| r.text.trim().to_string()).collect();
        assert_eq!(texts[0], "Klaudometer · Max 5x");
        assert_eq!(texts[1], "5-hour (inner ring): 81% used, 19% left");
        assert!(texts[2].starts_with("Resets in 2h 0m (") && texts[2].ends_with("Runs out in ~42m"), "{}", texts[2]);
        assert_eq!(texts[3], "7-day (outer ring): 47% used, 53% left");
        assert!(texts.contains(&"Extra usage: $12.34 of $50.00 (25%)".to_string()));
        assert!(texts.contains(&"API spend: $42.75 of $100 (43%)".to_string()));
        assert_eq!(texts.last().unwrap(), "Updated just now");
        assert_eq!(s.label, "5h 81% · 7d 47%");
        assert_eq!(s.rings.len(), 2);
    }

    #[test]
    fn alert_ids_match_the_other_front_ends() {
        let usage = mock_usage();
        let settings = settings_with_api();
        let meters = build_meters(Some(&usage), &settings, NOW);
        let alerts = alert_requests(&meters, &settings, &usage, NOW);
        let ids: Vec<_> = alerts.iter().map(|a| a.id.as_str()).collect();
        let five_period = ((NOW + 7200.0) / 60.0).round() as i64;
        assert_eq!(ids, [format!("five_hour:warning@75:{five_period}"), format!("five_hour:pace:{five_period}")]);
        assert_eq!(alerts[1].title, "Claude 5-hour limit: on pace to run out");
        assert_eq!(alerts[1].body, "At this rate you'll hit it in ~42m, 1h 17m before it resets.");

        // Past the critical level only the critical alert fires, not warning too.
        let mut critical = usage.clone();
        critical.subscription.windows.get_mut("five_hour").unwrap().used = 95.0;
        let meters = build_meters(Some(&critical), &settings, NOW);
        let alerts = alert_requests(&meters, &settings, &critical, NOW);
        assert_eq!(alerts[0].id, format!("five_hour:critical@90:{five_period}"));
        assert_eq!((alerts[0].level.as_str(), alerts[0].body.as_str()), ("high", "Only 5% left. Resets in 2h 0m."));
    }

    #[test]
    fn cards_match_the_widget_wording() {
        let s = summary(Some(&mock_usage()), &settings_with_api(), NOW);
        let five = &s.cards[0];
        assert_eq!((five.title.as_str(), five.big.as_str(), five.sub.as_str()), ("5-hour window", "81%", "used"));
        assert_eq!((five.caption.as_str(), five.resets.as_str(), five.pace.as_str()), ("19% left", "Resets in 2h 0m", "Runs out in ~42m"));
        assert!(five.runs_out && (five.marker - 0.6).abs() < 1e-9 && five.segments == 5);
        let api = &s.cards[3];
        assert_eq!((api.big.as_str(), api.sub.as_str(), api.caption.as_str()), ("$42.75", "of $100", "57% left"));
        let extra = &s.cards[2];
        assert_eq!((extra.title.as_str(), extra.marker, extra.resets.as_str()), ("Extra usage", -1.0, ""));
        assert_eq!(s.footer, "Updated just now");

        let left = summary(Some(&mock_usage()), &Settings { show_remaining: true, ..settings_with_api() }, NOW);
        assert_eq!((left.cards[0].big.as_str(), left.cards[0].sub.as_str(), left.cards[0].caption.as_str()), ("19%", "left", "81% used"));
        assert_eq!((left.cards[3].big.as_str(), left.cards[3].sub.as_str()), ("$57.25", "left of $100"));
    }

    #[test]
    fn nothing_selected_falls_back_to_both_windows() {
        let none = Settings { show_five_hour: false, show_seven_day: false, show_extra: false, show_api: false, ..Settings::default() };
        assert_eq!(meter_ids(&none, None), ["five_hour", "seven_day"]);
        let meters = build_meters(None, &none, NOW);
        assert!(meters.iter().all(|m| !m.has_data));
        assert_eq!(summary(None, &none, NOW).rows.last().unwrap().text, "Loading…");
    }
}
