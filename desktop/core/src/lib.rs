//! Klaudometer's shared core for the desktop app (macOS, Windows and Linux).
//!
//! It mirrors the Linux helper, `package/contents/code/fetch_usage.py`: the
//! same endpoints and the same cache and alert files in the same places. On
//! Linux, every Klaudometer front end therefore shares one cache and one
//! record of which alerts have been sent.

pub mod cache;
pub mod claims;
pub mod credentials;
pub mod env;
pub mod fetch;
pub mod format;
pub mod meters;
pub mod model;
pub mod settings;
pub mod update;

pub use env::Env;
pub use fetch::{fetch, FetchError, Request};
pub use meters::{alert_requests, build_meters, summary, Alert, Card, Meter, Summary};
pub use model::Usage;
pub use settings::Settings;

/// Klaudometer's version, shared by every front end.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Seconds since the Unix epoch, as the cache files store them.
pub fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
