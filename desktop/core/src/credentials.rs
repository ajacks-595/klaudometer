//! Reading Claude Code's login, and reading and storing the Admin API key.
//! Klaudometer only reads Claude Code's login and never refreshes it itself:
//! refresh tokens rotate, and racing Claude Code for one could log it out.

use crate::env::Env;
use crate::fetch::FetchError;
use serde::Serialize;
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
    run(program, args, None, timeout_secs).filter(|(ok, out)| *ok && !out.is_empty()).map(|(_, out)| out)
}

/// Run a program, optionally writing `input` to its stdin, and return whether
/// it succeeded and its trimmed stdout. None if it can't start or times out.
fn run(program: &str, args: &[&str], input: Option<&str>, timeout_secs: u64) -> Option<(bool, String)> {
    use std::io::{Read, Write};
    let mut command = if cfg!(windows) && program.to_lowercase().ends_with(".cmd") {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(program);
        c
    } else {
        Command::new(program)
    };
    let stdin = if input.is_some() { Stdio::piped() } else { Stdio::null() };
    command.args(args).stdin(stdin).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().ok()?;
    if let (Some(input), Some(mut pipe)) = (input, child.stdin.take()) {
        // Dropping the pipe afterwards closes it, so the program sees the end.
        let _ = pipe.write_all(input.as_bytes());
    }
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                child.stdout.take()?.read_to_string(&mut out).ok()?;
                return Some((status.success(), out.trim().to_string()));
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
    #[cfg(any(target_os = "macos", windows))]
    {
        // Keychain item or credential: service "klaudometer", account "admin".
        keyring::Entry::new("klaudometer", "admin").ok()?.get_password().ok().filter(|k| !k.is_empty())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Same Secret Service attributes as the Linux helper.
        run_capture("secret-tool", &["lookup", "service", "klaudometer", "key", "admin"], 60)
    }
}

/// Where the Admin API key was found, for the settings window. It never
/// carries the key itself.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct KeyStatus {
    /// "environment", the key store's name (`KEY_STORE`), "file", or None.
    pub source: Option<String>,
    /// Something wrong with a key that is there, such as a key file others can read.
    pub problem: Option<String>,
}

pub fn admin_key_status(env: &Env) -> KeyStatus {
    match load_admin_key(env) {
        Ok((_, source)) => KeyStatus { source: Some(source.to_string()), problem: None },
        Err(_) => KeyStatus { source: None, problem: file_admin_key(env).err().map(|e| e.message) },
    }
}

/// A pasted Admin API key, trimmed, or why it isn't one.
pub fn clean_admin_key(key: &str) -> Result<String, String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("Paste your Admin API key first.".into());
    }
    if !key.starts_with("sk-ant-admin") {
        return Err("That isn't an Admin API key. Admin keys start with sk-ant-admin.".into());
    }
    // Something must follow the "sk-ant-admin01-" prefix.
    if key.len() <= 15 || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("That key looks incomplete, or has extra characters in it.".into());
    }
    Ok(key.to_string())
}

/// Save the Admin API key in the system's key store and return where it
/// went. On Linux without a working wallet, it goes into a key file only
/// you can read instead.
pub fn store_admin_key(env: &Env, key: &str) -> Result<&'static str, String> {
    let key = clean_admin_key(key)?;
    #[cfg(any(target_os = "macos", windows))]
    {
        let _ = env;
        keyring::Entry::new("klaudometer", "admin")
            .and_then(|entry| entry.set_password(&key))
            .map_err(|e| format!("Couldn't save the key in the {KEY_STORE}: {e}"))?;
        Ok(KEY_STORE)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // secret-tool reads the secret from stdin, so it never appears in a command line.
        let args = ["store", "--label=Klaudometer Admin API key", "service", "klaudometer", "key", "admin"];
        if matches!(run("secret-tool", &args, Some(&key), 60), Some((true, _))) {
            return Ok(KEY_STORE);
        }
        write_key_file(env, &key)?;
        Ok("file")
    }
}

/// Delete the Admin API key from the key store and the key file. A key in
/// $ANTHROPIC_ADMIN_KEY stays, since it isn't Klaudometer's to remove.
pub fn remove_admin_key(env: &Env) -> Result<(), String> {
    #[cfg(any(target_os = "macos", windows))]
    match keyring::Entry::new("klaudometer", "admin").and_then(|entry| entry.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => return Err(format!("Couldn't remove the key from the {KEY_STORE}: {e}")),
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = run("secret-tool", &["clear", "service", "klaudometer", "key", "admin"], None, 60);
    let path = env.admin_key_file();
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Couldn't delete {}: {e}", path.display())),
        _ => Ok(()),
    }
}

/// Write the key file, readable only by you.
#[cfg(all(unix, not(target_os = "macos")))]
fn write_key_file(env: &Env, key: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = env.admin_key_file();
    let failed = |e: std::io::Error| format!("Couldn't save the key in {}: {e}", path.display());
    std::fs::create_dir_all(&env.config_dir).map_err(failed)?;
    // Start afresh, so an old file's looser permissions can't carry over.
    let _ = std::fs::remove_file(&path);
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).map_err(failed)?;
    writeln!(file, "{key}").map_err(failed)
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

    #[test]
    fn cleans_pasted_keys() {
        let key = "sk-ant-admin01-AbC_123-xyz789";
        assert_eq!(clean_admin_key(&format!("  {key}\n")).as_deref(), Ok(key));
        assert!(clean_admin_key("").unwrap_err().contains("Paste"));
        assert!(clean_admin_key("sk-ant-api03-AbC_123-xyz789").unwrap_err().contains("isn't an Admin API key"));
        assert!(clean_admin_key("sk-ant-admin01-AbC 123-xyz").unwrap_err().contains("incomplete"));
        assert!(clean_admin_key("sk-ant-admin01-").unwrap_err().contains("incomplete"));
        assert!(clean_admin_key("sk-ant-admin01-fake").is_ok());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn key_file_is_written_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("klaudometer-keyfile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let env = Env { config_dir: dir.clone(), admin_key: None, ..Env::system() };
        // An old file anyone could read must not pass its permissions on.
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(env.admin_key_file(), "old").unwrap();
        std::fs::set_permissions(env.admin_key_file(), std::fs::Permissions::from_mode(0o644)).unwrap();
        write_key_file(&env, "sk-ant-admin01-test").unwrap();
        let mode = std::fs::metadata(env.admin_key_file()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(file_admin_key(&env).unwrap().as_deref(), Some("sk-ant-admin01-test"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
