//! What gets fetched and cached. The JSON shapes match the Linux helper's
//! cache files exactly, so both can read what the other wrote.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One usage window: percent used, when it resets (Unix seconds) and how
/// long the window is, in seconds.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Window {
    pub used: f64,
    pub resets_at: Option<f64>,
    pub length: f64,
}

/// Extra usage: credits spent past the plan's limits, when it's turned on.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Extra {
    pub used: Option<f64>,
    pub limit: Option<f64>,
    pub percent: Option<f64>,
    #[serde(default)]
    pub currency: String,
}

/// The subscription result: the 5-hour and 7-day windows and extra usage.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Subscription {
    pub fetched_at: f64,
    pub plan: String,
    pub windows: BTreeMap<String, Window>,
    pub extra: Option<Extra>,
    pub stale: bool,
    pub error: Option<String>,
    pub next_attempt_at: f64,
}

/// Claude API spend for the current month (UTC), in currency units.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ApiSpend {
    pub spent: Option<f64>,
    pub today: Option<f64>,
    pub currency: String,
    pub month_start: f64,
    pub next_reset: f64,
    pub fetched_at: f64,
    pub stale: bool,
    pub error: Option<String>,
    pub next_attempt_at: f64,
}

/// Everything one refresh produced: the subscription result (empty when it
/// wasn't asked for) and API spend when it was.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    #[serde(flatten)]
    pub subscription: Subscription,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<ApiSpend>,
}
