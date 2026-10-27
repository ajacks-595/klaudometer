//! The shared cache: one file per source plus a lock, so several Klaudometer
//! front ends on one machine make a single request per refresh interval.

use crate::env::Env;
use crate::fetch::FetchError;
use crate::model::{ApiSpend, Subscription};
use serde::{de::DeserializeOwned, Serialize};
use std::fs::{self, File};
use std::path::Path;

/// A cached result that can say how old it is and be marked as stale.
pub trait Cacheable: Serialize + DeserializeOwned {
    fn fetched_at(&self) -> f64;
    fn has_error(&self) -> bool;
    fn next_attempt_at(&self) -> f64;
    fn mark_stale(&mut self, error: String, next_attempt_at: f64);
}

macro_rules! cacheable {
    ($type:ty) => {
        impl Cacheable for $type {
            fn fetched_at(&self) -> f64 {
                self.fetched_at
            }
            fn has_error(&self) -> bool {
                self.error.is_some()
            }
            fn next_attempt_at(&self) -> f64 {
                self.next_attempt_at
            }
            fn mark_stale(&mut self, error: String, next_attempt_at: f64) {
                self.stale = true;
                self.error = Some(error);
                self.next_attempt_at = next_attempt_at;
            }
        }
    };
}

cacheable!(Subscription);
cacheable!(ApiSpend);

/// Run `f` holding the cache lock. If the lock can't be taken (an unwritable
/// cache folder, say), run it anyway: a duplicate request beats no data.
pub fn with_lock<T>(env: &Env, f: impl FnOnce() -> T) -> T {
    let _ = fs::create_dir_all(&env.cache_dir);
    let lock = File::create(env.lock_file()).ok();
    let locked = lock.as_ref().map(|l| l.lock().is_ok()).unwrap_or(false);
    let result = f();
    if locked {
        if let Some(l) = &lock {
            let _ = l.unlock();
        }
    }
    result
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// Write via a temporary file so readers never see half a file.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, serde_json::to_vec(value)?)?;
    fs::rename(&tmp, path)
}

/// Return a cached result younger than `max_age` seconds, or fetch and cache
/// a new one. While backing off after an error, the cache is returned as is.
/// On failure the last good result comes back marked stale. Call under the
/// lock.
pub fn cached<T: Cacheable>(
    path: &Path,
    max_age: f64,
    now: f64,
    fetch: impl FnOnce(f64) -> Result<T, FetchError>,
    empty: T,
) -> T {
    let cache: Option<T> = read_json(path);
    let reuse = cache.as_ref().is_some_and(|c| {
        let fresh = !c.has_error() && now - c.fetched_at() < max_age;
        let backing_off = now < c.next_attempt_at();
        max_age > 0.0 && (fresh || backing_off)
    });
    let cache = match cache {
        Some(cache) if reuse => return cache,
        other => other,
    };
    let result = match fetch(now) {
        Ok(result) => result,
        Err(e) => {
            let mut result = cache.unwrap_or(empty);
            result.mark_stale(e.message, now + e.retry_after);
            result
        }
    };
    let _ = write_json(path, &result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::FetchError;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("klaudometer-cache-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample(fetched_at: f64) -> ApiSpend {
        ApiSpend { spent: Some(1.0), fetched_at, ..Default::default() }
    }

    #[test]
    fn fresh_cache_is_reused_and_old_cache_refetched() {
        let path = temp_dir("fresh").join("api.json");
        write_json(&path, &sample(1000.0)).unwrap();
        let reused = cached(&path, 600.0, 1100.0, |_| -> Result<ApiSpend, FetchError> { panic!("fetched a fresh cache") }, ApiSpend::default());
        assert_eq!(reused.fetched_at, 1000.0);
        let refetched = cached(&path, 600.0, 2000.0, |now| Ok(sample(now)), ApiSpend::default());
        assert_eq!(refetched.fetched_at, 2000.0);
        assert_eq!(read_json::<ApiSpend>(&path).unwrap().fetched_at, 2000.0);
    }

    #[test]
    fn failure_keeps_last_good_result_and_backs_off() {
        let path = temp_dir("stale").join("api.json");
        write_json(&path, &sample(1000.0)).unwrap();
        let failed = cached(&path, 0.0, 2000.0, |_| Err(FetchError::new("Offline", 60.0)), ApiSpend::default());
        assert_eq!(failed.spent, Some(1.0));
        assert!(failed.stale);
        assert_eq!(failed.error.as_deref(), Some("Offline"));
        // Within the back-off window the cache is served without fetching.
        let backing_off = cached(&path, 600.0, 2030.0, |_| -> Result<ApiSpend, FetchError> { panic!("fetched while backing off") }, ApiSpend::default());
        assert!(backing_off.stale);
    }
}
