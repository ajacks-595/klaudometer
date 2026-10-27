//! The desktop app's settings, stored as JSON in the config folder. Field
//! names and defaults match the Linux tray icon's tray.ini.

use crate::fetch::Request;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub refresh_minutes: u32,
    pub show_five_hour: bool,
    pub show_seven_day: bool,
    /// Only shown while extra usage is turned on for the account.
    pub show_extra: bool,
    pub show_api: bool,
    pub api_budget: f64,
    /// Numbers show usage left instead of usage used.
    pub show_remaining: bool,
    /// Text next to the tray icon where supported: "full", "compact" or "off".
    pub label: String,
    /// claude, ocean, sunset, forest or mono; the *_color fields override single colours.
    pub theme: String,
    pub five_hour_color: String,
    pub seven_day_color: String,
    pub spend_color: String,
    pub warning_color: String,
    pub critical_color: String,
    /// Warning and critical levels, in percent, for colours and notifications.
    pub warning: f64,
    pub critical: f64,
    pub color_by_level: bool,
    pub show_pace: bool,
    pub notify_warning: bool,
    pub notify_critical: bool,
    pub notify_pace: bool,
    pub notify_reset: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            refresh_minutes: 3,
            show_five_hour: true,
            show_seven_day: true,
            show_extra: true,
            show_api: false,
            api_budget: 100.0,
            show_remaining: false,
            label: "compact".into(),
            theme: "claude".into(),
            five_hour_color: String::new(),
            seven_day_color: String::new(),
            spend_color: String::new(),
            warning_color: String::new(),
            critical_color: String::new(),
            warning: 75.0,
            critical: 90.0,
            color_by_level: true,
            show_pace: true,
            notify_warning: true,
            notify_critical: true,
            notify_pace: false,
            notify_reset: false,
        }
    }
}

impl Settings {
    /// Settings from `path`, or the defaults if it's missing or unreadable.
    pub fn load(path: &Path) -> Settings {
        crate::cache::read_json(path).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::cache::write_json(path, self)
    }

    pub fn refresh_seconds(&self) -> f64 {
        f64::from(self.refresh_minutes.max(1)) * 60.0
    }

    /// Subscription data is only fetched when something shows it. With nothing
    /// at all ticked, the 5-hour and 7-day windows show.
    pub fn wants_subscription(&self) -> bool {
        self.show_five_hour || self.show_seven_day || self.show_extra || !self.show_api
    }

    /// What a refresh should fetch. Cost data moves slowly, so API spend is
    /// checked at most every 10 minutes unless forced.
    pub fn request(&self, force: bool) -> Request {
        Request {
            subscription: self.wants_subscription(),
            api: self.show_api,
            max_age: if force { 0.0 } else { (self.refresh_seconds() - 20.0).max(30.0) },
            api_max_age: if force { 0.0 } else { 600.0 },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_defaults() {
        let s: Settings = serde_json::from_str(r#"{"show_api": true, "warning": 60}"#).unwrap();
        assert!(s.show_api);
        assert_eq!(s.warning, 60.0);
        assert_eq!(s.critical, 90.0);
        assert_eq!(s.theme, "claude");
    }

    #[test]
    fn requests() {
        let s = Settings::default();
        assert_eq!(s.request(false), Request { subscription: true, api: false, max_age: 160.0, api_max_age: 600.0 });
        let api_only = Settings { show_five_hour: false, show_seven_day: false, show_extra: false, show_api: true, ..s };
        let r = api_only.request(true);
        assert!(!r.subscription && r.api && r.max_age == 0.0 && r.api_max_age == 0.0);
    }
}
