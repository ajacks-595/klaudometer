//! End-to-end fetching against a local stand-in for Anthropic's API: the
//! usage endpoint with a Claude Code login, the cost report over two pages
//! with an Admin key, the shared cache, and error handling. The same stand-in
//! answers the update check's question to GitHub.

use klaudometer_core::{fetch, update, Env, Request};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const OAUTH_TOKEN: &str = "fake-oauth-token";
const ADMIN_KEY: &str = "sk-ant-admin01-fake";

/// Serve canned responses on a random local port; returns the base URL and
/// a counter of requests served.
fn start_mock() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let served = Arc::new(AtomicUsize::new(0));
    let counter = served.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            counter.fetch_add(1, Ordering::SeqCst);
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut headers = Vec::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                headers.push(line.trim().to_lowercase());
            }
            let path = request_line.split_whitespace().nth(1).unwrap_or("").to_string();
            let (status, body) = respond(&path, &headers);
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (base, served)
}

fn respond(path: &str, headers: &[String]) -> (&'static str, String) {
    let has = |h: &str| headers.iter().any(|x| x == h);
    let now = klaudometer_core::now();
    let iso = |t: f64| chrono::DateTime::from_timestamp(t as i64, 0).unwrap().to_rfc3339();
    if path.starts_with("/api/oauth/usage") {
        if !has(&format!("authorization: bearer {OAUTH_TOKEN}")) {
            return ("401 Unauthorized", "{}".into());
        }
        let body = serde_json::json!({
            "five_hour": {"utilization": 81.0, "resets_at": iso(now + 7200.0)},
            "seven_day": {"utilization": 47.0, "resets_at": iso(now + 3.0 * 86400.0)},
            "spend": {"enabled": true, "percent": 24.68,
                      "used": {"amount_minor": 1234, "currency": "USD", "exponent": 2},
                      "limit": {"amount_minor": 5000, "currency": "USD", "exponent": 2}}
        });
        return ("200 OK", body.to_string());
    }
    if path.starts_with("/v1/organizations/cost_report") {
        if !has(&format!("x-api-key: {ADMIN_KEY}")) {
            return ("401 Unauthorized", "{}".into());
        }
        assert!(path.contains("bucket_width=1d") && path.contains("starting_at=") && has("anthropic-version: 2023-06-01"));
        let today = chrono::Utc::now().format("%Y-%m-%dT00:00:00Z").to_string();
        let bucket = |start: &str, amounts: &[&str]| {
            serde_json::json!({"starting_at": start, "ending_at": start,
                               "results": amounts.iter().map(|a| serde_json::json!({"amount": a, "currency": "USD"})).collect::<Vec<_>>()})
        };
        let body = if path.contains("page=p2") {
            serde_json::json!({"data": [bucket(&today, &["1200", "50"])], "has_more": false, "next_page": null})
        } else {
            // Earlier days, dated so they never collide with today.
            serde_json::json!({"data": [bucket("2000-01-01T00:00:00Z", &["1000.00", "250.5"]), bucket("2000-01-02T00:00:00Z", &["1774.5"])],
                               "has_more": true, "next_page": "p2"})
        };
        return ("200 OK", body.to_string());
    }
    if path == "/repos/ajacks-595/klaudometer/releases/latest" {
        assert!(has("x-github-api-version: 2022-11-28"));
        let body = serde_json::json!({"tag_name": "v9.9.9", "html_url": "https://github.com/ajacks-595/klaudometer/releases/tag/v9.9.9"});
        return ("200 OK", body.to_string());
    }
    ("404 Not Found", "{}".into())
}

/// A throwaway environment with a fake Claude Code login and Admin key.
fn test_env(name: &str, base: &str) -> Env {
    let root: PathBuf = std::env::temp_dir().join(format!("klaudometer-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let claude_dir = root.join("claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let expires_ms = (klaudometer_core::now() + 86400.0) * 1000.0;
    std::fs::write(
        claude_dir.join(".credentials.json"),
        format!(r#"{{"claudeAiOauth":{{"accessToken":"{OAUTH_TOKEN}","expiresAt":{expires_ms},"subscriptionType":"max","rateLimitTier":"default_claude_max_5x"}}}}"#),
    )
    .unwrap();
    Env {
        api_base: base.to_string(),
        github_api: base.to_string(),
        claude_dir,
        cache_dir: root.join("cache"),
        config_dir: root.join("config"),
        admin_key: Some(ADMIN_KEY.into()),
        use_keychain: false,
    }
}

fn everything(force: bool) -> Request {
    let age = if force { 0.0 } else { 600.0 };
    Request { subscription: true, api: true, max_age: age, api_max_age: age }
}

#[test]
fn fetches_usage_and_spend_then_serves_the_cache() {
    let (base, served) = start_mock();
    let env = test_env("all", &base);
    let usage = fetch(&env, &everything(true), klaudometer_core::now());

    let sub = &usage.subscription;
    assert_eq!(sub.error, None);
    assert_eq!(sub.plan, "Max 5x");
    assert_eq!(sub.windows["five_hour"].used, 81.0);
    assert_eq!(sub.windows["seven_day"].length, 604800.0);
    let extra = sub.extra.as_ref().unwrap();
    assert_eq!((extra.used, extra.limit), (Some(12.34), Some(50.0)));

    let api = usage.api.as_ref().unwrap();
    assert_eq!(api.error, None);
    assert!((api.spent.unwrap() - 42.75).abs() < 1e-9, "two pages summed: {:?}", api.spent);
    assert!((api.today.unwrap() - 12.5).abs() < 1e-9);
    assert!(api.next_reset > api.month_start);
    // Usage + two cost pages.
    assert_eq!(served.load(Ordering::SeqCst), 3);

    // Within max-age, a second fetch is served from the shared cache files.
    let again = fetch(&env, &everything(false), klaudometer_core::now());
    assert_eq!(again, usage);
    assert_eq!(served.load(Ordering::SeqCst), 3);
    assert!(env.usage_cache().exists() && env.api_cache().exists());
}

#[test]
fn rejected_admin_key_keeps_last_good_spend() {
    let (base, _) = start_mock();
    let mut env = test_env("badkey", &base);
    let api_only = |force| Request { subscription: false, ..everything(force) };
    let good = fetch(&env, &api_only(true), klaudometer_core::now()).api.unwrap();
    env.admin_key = Some("sk-ant-admin01-wrong".into());
    let bad = fetch(&env, &api_only(true), klaudometer_core::now());
    let api = bad.api.unwrap();
    assert_eq!(api.error.as_deref(), Some("The Admin API key was rejected."));
    assert!(api.stale);
    assert_eq!(api.spent, good.spent);
    // Nothing subscription-related was asked for, so there's no error there.
    assert_eq!(bad.subscription.error, None);
    assert!(bad.subscription.windows.is_empty());
}

#[test]
fn missing_login_is_explained() {
    let (base, _) = start_mock();
    let env = test_env("nologin", &base);
    std::fs::remove_file(env.claude_dir.join(".credentials.json")).unwrap();
    let usage = fetch(&env, &Request { api: false, ..everything(true) }, klaudometer_core::now());
    assert_eq!(usage.subscription.error.as_deref(), Some("Not signed in to Claude Code. Run `claude` and log in."));
    assert!(usage.subscription.stale);
}

#[test]
fn reads_caches_written_by_the_linux_helper() {
    // A usage.json exactly as fetch_usage.py writes it.
    let (base, _) = start_mock();
    let env = test_env("compat", &base);
    std::fs::create_dir_all(&env.cache_dir).unwrap();
    let now = klaudometer_core::now();
    std::fs::write(
        env.usage_cache(),
        format!(
            r#"{{"fetched_at": {now}, "plan": "Pro", "windows": {{"five_hour": {{"used": 28.0, "resets_at": {}, "length": 18000}}, "seven_day": {{"used": 46.0, "resets_at": null, "length": 604800}}}}, "extra": null, "stale": false, "error": null, "next_attempt_at": 0}}"#,
            now + 3600.0
        ),
    )
    .unwrap();
    let usage = fetch(&env, &Request { api: false, ..everything(false) }, now);
    assert_eq!(usage.subscription.plan, "Pro");
    assert_eq!(usage.subscription.windows["five_hour"].used, 28.0);
    assert_eq!(usage.subscription.windows["seven_day"].resets_at, None);
}

#[test]
fn update_check_finds_a_newer_release_and_remembers_it() {
    let (base, served) = start_mock();
    let env = test_env("update", &base);
    let now = klaudometer_core::now();
    let found = update::check(&env, 3600.0, now);
    assert_eq!(found.latest.as_deref(), Some("9.9.9"));
    let offer = update::available(&found, "1.1.0").unwrap();
    assert_eq!(offer.url, "https://github.com/ajacks-595/klaudometer/releases/tag/v9.9.9");
    assert!(update::available(&found, "9.9.9").is_none());

    // Within a day the saved check is reused, so a restart doesn't ask again.
    assert_eq!(update::check(&env, 86400.0, now + 600.0), found);
    assert_eq!(served.load(Ordering::SeqCst), 1);

    // When GitHub can't be reached, the last good answer stays, and it tries
    // again after an hour rather than a day.
    let offline = Env { github_api: "http://127.0.0.1:9".into(), ..env.clone() };
    let failed = update::check(&offline, 86400.0, now + 90000.0);
    assert!(failed.error.is_some());
    assert_eq!(failed.latest.as_deref(), Some("9.9.9"));
    assert_eq!(update::check(&offline, 86400.0, now + 90600.0), failed);
    assert_ne!(update::check(&offline, 86400.0, now + 94000.0).checked_at, failed.checked_at);
}
