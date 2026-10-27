//! The shared record of alerts already sent. Every front end asks here before
//! notifying, so each alert pops up once however many are running.

use crate::cache::{read_json, with_lock, write_json};
use crate::env::Env;
use std::collections::BTreeMap;

/// Alert IDs embed their window's reset time or month, so they can't recur
/// after a little over a month.
const CLAIM_TTL_SECONDS: f64 = 35.0 * 86400.0;

/// Record `ids` and return the ones no front end had claimed before.
pub fn claim(env: &Env, ids: &[String], now: f64) -> Vec<String> {
    with_lock(env, || {
        let path = env.claims_file();
        let mut claims: BTreeMap<String, f64> = read_json(&path).unwrap_or_default();
        claims.retain(|_, at| now - *at < CLAIM_TTL_SECONDS);
        let mut fresh = Vec::new();
        for id in ids {
            if !claims.contains_key(id) && !fresh.contains(id) {
                fresh.push(id.clone());
            }
        }
        for id in &fresh {
            claims.insert(id.clone(), now);
        }
        let _ = write_json(&path, &claims);
        fresh
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_alert_is_claimed_once_and_old_claims_expire() {
        let dir = std::env::temp_dir().join(format!("klaudometer-claims-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let env = Env { cache_dir: dir.clone(), ..crate::env::Env::system() };
        let ids = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        assert_eq!(claim(&env, &ids(&["a", "b", "b"]), 1000.0), ids(&["a", "b"]));
        assert_eq!(claim(&env, &ids(&["a", "c"]), 1001.0), ids(&["c"]));
        // Long after the TTL, "a" may be claimed again.
        assert_eq!(claim(&env, &ids(&["a"]), 1000.0 + CLAIM_TTL_SECONDS + 1.0), ids(&["a"]));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
