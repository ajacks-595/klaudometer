//! Talking to Anthropic: the subscription usage endpoint (with Claude Code's
//! login) and the Console cost report (with an Admin API key).

use crate::cache::{cached, with_lock};
use crate::credentials::{load_admin_key, load_claude_login, refresh_via_cli, ClaudeLogin};
use crate::env::Env;
use crate::model::{ApiSpend, Extra, Subscription, Usage, Window};
use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

pub const USER_AGENT: &str = concat!("klaudometer-desktop/", env!("CARGO_PKG_VERSION"));
const ERROR_RETRY_SECONDS: f64 = 60.0;
/// Usage windows the subscription endpoint reports, with their lengths.
const WINDOWS: [(&str, f64); 2] = [("five_hour", 5.0 * 3600.0), ("seven_day", 7.0 * 86400.0)];

/// A failed fetch: a message for people, how long to wait before trying
/// again, and whether it was a credentials problem.
#[derive(Clone, Debug, PartialEq)]
pub struct FetchError {
    pub message: String,
    pub retry_after: f64,
    pub auth: bool,
}

impl FetchError {
    pub fn new(message: &str, retry_after: f64) -> FetchError {
        FetchError { message: message.to_string(), retry_after, auth: false }
    }

    fn auth(message: &str) -> FetchError {
        FetchError { message: message.to_string(), retry_after: 120.0, auth: true }
    }
}

/// What one refresh should fetch, and how old a cached result may be (in
/// seconds; 0 forces a fetch).
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub subscription: bool,
    pub api: bool,
    pub max_age: f64,
    pub api_max_age: f64,
}

/// Fetch (or reuse from the shared cache) everything `request` asks for.
pub fn fetch(env: &Env, request: &Request, now: f64) -> Usage {
    with_lock(env, || Usage {
        subscription: if request.subscription {
            cached(&env.usage_cache(), request.max_age, now, |n| fetch_subscription(env, n), Subscription::default())
        } else {
            Subscription::default()
        },
        api: request.api.then(|| {
            cached(&env.api_cache(), request.api_max_age, now, |n| fetch_api_spend(env, n), ApiSpend::default())
        }),
    })
}

/// GET a JSON document. `auth_errors` maps HTTP status codes to messages for
/// credential problems.
pub(crate) fn request_json(url: &str, headers: &[(&str, &str)], auth_errors: &[(u16, &str)]) -> Result<Value, FetchError> {
    let mut request = ureq::get(url)
        .timeout(Duration::from_secs(15))
        .set("Accept", "application/json")
        .set("User-Agent", USER_AGENT);
    for (name, value) in headers {
        request = request.set(name, value);
    }
    match request.call() {
        Ok(response) => serde_json::from_reader(response.into_reader())
            .map_err(|_| FetchError::new("Unexpected response from the Anthropic API.", ERROR_RETRY_SECONDS)),
        Err(ureq::Error::Status(code, response)) => {
            if let Some((_, message)) = auth_errors.iter().find(|(c, _)| *c == code) {
                return Err(FetchError::auth(message));
            }
            if code == 429 {
                let retry = response.header("Retry-After").and_then(|r| r.parse().ok()).unwrap_or(300.0);
                return Err(FetchError::new("Rate limited by the Anthropic API, retrying later.", retry));
            }
            Err(FetchError::new(&format!("Anthropic API error (HTTP {code})."), ERROR_RETRY_SECONDS))
        }
        Err(ureq::Error::Transport(_)) => Err(FetchError::new("Offline, showing last known values.", ERROR_RETRY_SECONDS)),
    }
}

// --- Subscription limits ----------------------------------------------------

const LOGIN_EXPIRED: &str = "Login expired. Open Claude Code to refresh it.";

fn request_usage(env: &Env, login: &ClaudeLogin) -> Result<Value, FetchError> {
    let bearer = format!("Bearer {}", login.access_token);
    request_json(
        &format!("{}/api/oauth/usage", env.api_base),
        &[("Authorization", &bearer), ("anthropic-beta", "oauth-2025-04-20")],
        &[(401, LOGIN_EXPIRED)],
    )
}

fn fetch_raw(env: &Env, now: f64) -> Result<(Value, ClaudeLogin), FetchError> {
    let mut login = load_claude_login(env)?;
    if login.expired(now) {
        refresh_via_cli();
        login = load_claude_login(env)?;
        if login.expired(now) {
            return Err(FetchError::auth(LOGIN_EXPIRED));
        }
    }
    match request_usage(env, &login) {
        Err(e) if e.auth => {
            refresh_via_cli();
            let login = load_claude_login(env)?;
            Ok((request_usage(env, &login)?, login))
        }
        result => Ok((result?, login)),
    }
}

pub fn to_epoch(iso: &str) -> Option<f64> {
    DateTime::parse_from_rfc3339(iso).ok().map(|t| t.timestamp_micros() as f64 / 1e6)
}

/// {"amount_minor": 1234, "exponent": 2} → 12.34.
fn money(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    let minor = value.get("amount_minor")?.as_f64()?;
    let exponent = value.get("exponent").and_then(Value::as_i64).unwrap_or(0);
    Some(minor / 10f64.powi(exponent as i32))
}

/// Extra usage (credits past the plan's limits), or None while it's off.
pub fn extra_usage(raw: &Value) -> Option<Extra> {
    let empty = Value::Object(Default::default());
    let spend = raw.get("spend").filter(|s| s.is_object()).unwrap_or(&empty);
    let extra = raw.get("extra_usage").filter(|e| e.is_object()).unwrap_or(&empty);
    let enabled = |v: &Value, key: &str| v.get(key).and_then(Value::as_bool).unwrap_or(false);
    if !(enabled(spend, "enabled") || enabled(extra, "is_enabled")) {
        return None;
    }
    let used = money(spend.get("used"));
    let limit = money(spend.get("limit"));
    let percent = spend
        .get("percent")
        .and_then(Value::as_f64)
        .or_else(|| extra.get("utilization").and_then(Value::as_f64))
        .or_else(|| match (used, limit) {
            (Some(u), Some(l)) if l > 0.0 => Some(u / l * 100.0),
            _ => None,
        });
    let currency = spend.pointer("/used/currency").and_then(Value::as_str).unwrap_or("USD").to_string();
    Some(Extra { used, limit, percent, currency })
}

pub fn fetch_subscription(env: &Env, now: f64) -> Result<Subscription, FetchError> {
    let (raw, login) = fetch_raw(env, now)?;
    let mut windows = BTreeMap::new();
    for (key, length) in WINDOWS {
        let Some(w) = raw.get(key) else { continue };
        if let Some(used) = w.get("utilization").and_then(Value::as_f64) {
            let resets_at = w.get("resets_at").and_then(Value::as_str).and_then(to_epoch);
            windows.insert(key.to_string(), Window { used, resets_at, length });
        }
    }
    if windows.is_empty() {
        return Err(FetchError::new("The usage API returned no 5-hour or 7-day data.", ERROR_RETRY_SECONDS));
    }
    Ok(Subscription {
        fetched_at: now,
        plan: login.plan_name(),
        windows,
        extra: extra_usage(&raw),
        stale: false,
        error: None,
        next_attempt_at: 0.0,
    })
}

// --- Claude API spend ------------------------------------------------------

/// Percent-encode a query value.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn fetch_api_spend(env: &Env, now: f64) -> Result<ApiSpend, FetchError> {
    let (key, _) = load_admin_key(env)?;
    let today = Utc.timestamp_opt(now as i64, 0).single().unwrap_or_default().date_naive();
    let month_start = Utc.with_ymd_and_hms(today.year(), today.month(), 1, 0, 0, 0).unwrap();
    let next_month = if today.month() == 12 {
        Utc.with_ymd_and_hms(today.year() + 1, 1, 1, 0, 0, 0).unwrap()
    } else {
        Utc.with_ymd_and_hms(today.year(), today.month() + 1, 1, 0, 0, 0).unwrap()
    };
    let today_start = today.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp() as f64;
    let starting_at = month_start.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    // Amounts are decimal strings in cents.
    let (mut total, mut today_total) = (0.0, 0.0);
    let mut page: Option<String> = None;
    for _ in 0..5 {
        let mut url = format!(
            "{}/v1/organizations/cost_report?starting_at={}&bucket_width=1d&limit=31",
            env.api_base,
            encode(&starting_at)
        );
        if let Some(p) = &page {
            url += &format!("&page={}", encode(p));
        }
        let report = request_json(
            &url,
            &[("x-api-key", &key), ("anthropic-version", "2023-06-01")],
            &[(401, "The Admin API key was rejected."), (403, "This key can't read cost data. Use an Admin API key.")],
        )?;
        for bucket in report.get("data").and_then(Value::as_array).into_iter().flatten() {
            let cents: f64 = bucket
                .get("results")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|r| r.get("amount").and_then(Value::as_str).and_then(|a| a.parse::<f64>().ok()))
                .sum();
            total += cents;
            if bucket.get("starting_at").and_then(Value::as_str).and_then(to_epoch) == Some(today_start) {
                today_total += cents;
            }
        }
        page = report.get("next_page").and_then(Value::as_str).map(str::to_string);
        if !report.get("has_more").and_then(Value::as_bool).unwrap_or(false) || page.is_none() {
            break;
        }
    }
    Ok(ApiSpend {
        spent: Some(total / 100.0),
        today: Some(today_total / 100.0),
        currency: "USD".into(),
        month_start: month_start.timestamp() as f64,
        next_reset: next_month.timestamp() as f64,
        fetched_at: now,
        stale: false,
        error: None,
        next_attempt_at: 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_reset_times() {
        assert_eq!(to_epoch("2026-10-27T01:49:59.896835+00:00"), Some(1793065799.896835));
        assert_eq!(to_epoch("2026-10-01T00:00:00Z"), Some(1790812800.0));
        assert_eq!(to_epoch("not a date"), None);
    }

    #[test]
    fn extra_usage_only_when_turned_on() {
        assert_eq!(extra_usage(&json!({"extra_usage": {"is_enabled": false}, "spend": {"enabled": false}})), None);
        let extra = extra_usage(&json!({
            "spend": {"enabled": true, "percent": 24.68,
                      "used": {"amount_minor": 1234, "currency": "USD", "exponent": 2},
                      "limit": {"amount_minor": 5000, "currency": "USD", "exponent": 2}}
        }))
        .unwrap();
        assert_eq!((extra.used, extra.limit, extra.percent), (Some(12.34), Some(50.0), Some(24.68)));
        // Percent falls back to used / limit.
        let extra = extra_usage(&json!({
            "spend": {"enabled": true, "used": {"amount_minor": 500, "exponent": 2}, "limit": {"amount_minor": 2000, "exponent": 2}}
        }))
        .unwrap();
        assert_eq!(extra.percent, Some(25.0));
    }

    #[test]
    fn encodes_query_values() {
        assert_eq!(encode("2026-10-01T00:00:00Z"), "2026-10-01T00%3A00%3A00Z");
        assert_eq!(encode("page_abc="), "page_abc%3D");
    }
}
