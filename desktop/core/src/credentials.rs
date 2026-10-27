//! Reading Claude Code's login and the Admin API key. Klaudometer only reads
//! Claude Code's login and never refreshes it itself: refresh tokens rotate,
//! and racing Claude Code for one could log it out.

use crate::env::Env;
use crate::fetch::FetchError;
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The parts of Claude Code's stored login that Klaudometer uses.
#[derive(Clone, Debug, PartialEq)]
pub struct ClaudeLogin {
    pub access_token: String,
    pub expires_at_ms: f64,
    pub subscription_type: String,
    pub rate_limit_tier: String,
}

impl ClaudeLogin {
    pub fn expired(&self, now: f64) -> bool {
        self.expires_at_ms / 1000.0 < now + 60.0
    }

    pub fn plan_name(&self) -> String {
        let kind = self.subscription_type.to_lowercase();
        if kind == "max" {
            for mult in ["20x", "5x"] {
                if self.rate_limit_tier.to_lowercase().contains(mult) {
                    return format!("Max {mult}");
                }
            }
        }
        let mut chars = kind.chars();
        chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
    }
}

/// Parse Claude Code's credentials JSON (`{"claudeAiOauth": {...}}`).
pub fn parse_login(text: &str) -> Option<ClaudeLogin> {
    let value: Value = serde_json::from_str(text).ok()?;
    let oauth = value.get("claudeAiOauth")?;
    let text = |key: &str| oauth.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    Some(ClaudeLogin {
        access_token: oauth.get("accessToken")?.as_str()?.to_string(),
        expires_at_ms: oauth.get("expiresAt").and_then(Value::as_f64).unwrap_or(0.0),
        subscription_type: text("subscriptionType"),
        rate_limit_tier: text("rateLimitTier"),
    })
}

/// Claude Code's login: the macOS Keychain first (where Claude Code keeps it
/// on a Mac), then `.credentials.json` in its config folder (Linux, Windows,
/// and the Mac fallback when the Keychain is locked).
pub fn load_claude_login(env: &Env) -> Result<ClaudeLogin, FetchError> {
    if env.use_keychain {
        let keychain = run_capture("security", &["find-generic-password", "-s", "Claude Code-credentials", "-w"], 30);
        if let Some(login) = keychain.as_deref().and_then(parse_login) {
            return Ok(login);
        }
    }
    std::fs::read_to_string(env.claude_dir.join(".credentials.json"))
        .ok()
        .and_then(|text| parse_login(&text))
        .ok_or_else(|| FetchError::new("Not signed in to Claude Code. Run `claude` and log in.", 300.0))
}

/// Ask the Claude Code CLI to refresh its own login (best effort).
pub fn refresh_via_cli() {
    if let Some(claude) = find_claude() {
        let _ = run_capture(&claude.to_string_lossy(), &["auth", "status"], 30);
    }
}

/// Where Claude Code's CLI usually lives. Apps started from the desktop often
/// have a PATH without the per-user install folders, so check those too.
fn find_claude() -> Option<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if cfg!(windows) {
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".bun").join("bin"));
        if let Some(appdata) = std::env::var_os("APPDATA") {
            dirs.push(PathBuf::from(appdata).join("npm"));
        }
    } else {
        for d in [".local/bin", ".claude/local", ".npm-global/bin", ".bun/bin", ".volta/bin", "bin"] {
            dirs.push(home.join(d));
        }
        dirs.push("/usr/local/bin".into());
        dirs.push("/opt/homebrew/bin".into());
        if let Ok(versions) = std::fs::read_dir(home.join(".nvm/versions/node")) {
            dirs.extend(versions.flatten().map(|v| v.path().join("bin")));
        }
    }
    let names: &[&str] = if cfg!(windows) { &["claude.exe", "claude.cmd"] } else { &["claude"] };
    dirs.iter().flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file())
}

/// Run a program and return its trimmed stdout if it succeeds within
/// `timeout_secs`. Never shows a console window on Windows.
pub(crate) fn run_capture(program: &str, args: &[&str], timeout_secs: u64) -> Option<String> {
    let mut command = if cfg!(windows) && program.to_lowercase().ends_with(".cmd") {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(program);
        c
    } else {
        Command::new(program)
    };
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                use std::io::Read;
                child.stdout.take()?.read_to_string(&mut out).ok()?;
                let out = out.trim().to_string();
                return (status.success() && !out.is_empty()).then_some(out);
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
}

/// Where a stored Admin API key lives on this system, for messages.
pub const KEY_STORE: &str = if cfg!(target_os = "macos") {
    "Keychain"
} else if cfg!(windows) {
    "Credential Manager"
} else {
    "wallet"
};

/// The Admin API key and where it came from: $ANTHROPIC_ADMIN_KEY, the
/// system's key store, or a private key file. The key is never printed or
/// cached.
pub fn load_admin_key(env: &Env) -> Result<(String, &'static str), FetchError> {
    if let Some(key) = &env.admin_key {
        return Ok((key.clone(), "environment"));
    }
    if let Some(key) = stored_admin_key() {
        return Ok((key, KEY_STORE));
    }
    if let Some(key) = file_admin_key(env)? {
        return Ok((key, "file"));
    }
    Err(FetchError::new("No Admin API key stored. See Klaudometer's Spending settings.", 300.0))
}

fn stored_admin_key() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        run_capture("security", &["find-generic-password", "-s", "klaudometer", "-a", "admin", "-w"], 60)
    }
    #[cfg(windows)]
    {
        keyring::Entry::new("klaudometer", "admin").ok()?.get_password().ok().filter(|k| !k.is_empty())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Same Secret Service attributes as the Linux helper.
        run_capture("secret-tool", &["lookup", "service", "klaudometer", "key", "admin"], 60)
    }
}

/// The key from the key file, refusing a file other users can read.
fn file_admin_key(env: &Env) -> Result<Option<String>, FetchError> {
    let path = env.admin_key_file();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).map(|m| m.permissions().mode()).unwrap_or(0);
        if mode & 0o077 != 0 {
            let shown = path.display();
            return Err(FetchError::new(&format!("{shown} must be private. Run: chmod 600 {shown}"), 300.0));
        }
    }
    let key = text.trim().to_string();
    Ok((!key.is_empty()).then_some(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_code_credentials() {
        let login = parse_login(
            r#"{"claudeAiOauth":{"accessToken":"tok","expiresAt":2000000,"subscriptionType":"max","rateLimitTier":"default_claude_max_20x"}}"#,
        )
        .unwrap();
        assert_eq!(login.access_token, "tok");
        assert_eq!(login.plan_name(), "Max 20x");
        assert!(login.expired(2000.0 - 30.0));
        assert!(!login.expired(1000.0));
        assert!(parse_login(r#"{"other":{}}"#).is_none());
    }

    #[test]
    fn plan_names() {
        let plan = |kind: &str, tier: &str| {
            ClaudeLogin { access_token: String::new(), expires_at_ms: 0.0, subscription_type: kind.into(), rate_limit_tier: tier.into() }
                .plan_name()
        };
        assert_eq!(plan("pro", "default_claude_ai"), "Pro");
        assert_eq!(plan("max", "default_claude_max_5x"), "Max 5x");
        assert_eq!(plan("", ""), "");
    }

    #[cfg(unix)]
    #[test]
    fn key_file_must_be_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("klaudometer-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let env = Env { config_dir: dir.clone(), admin_key: None, ..Env::system() };
        let path = env.admin_key_file();
        std::fs::write(&path, "sk-ant-admin01-test\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(file_admin_key(&env).unwrap_err().message.contains("must be private"));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(file_admin_key(&env).unwrap().as_deref(), Some("sk-ant-admin01-test"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
