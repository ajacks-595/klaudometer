use std::path::PathBuf;

/// Where Klaudometer reads and writes, and which API it talks to.
/// `Env::system()` builds it from the real machine; tests point it at
/// temporary folders and a local mock server instead.
#[derive(Clone, Debug)]
pub struct Env {
    pub api_base: String,
    /// GitHub's API, asked for the latest release by the update check.
    pub github_api: String,
    /// Claude Code's configuration folder: `~/.claude`, or $CLAUDE_CONFIG_DIR.
    pub claude_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub config_dir: PathBuf,
    /// An Admin API key from $ANTHROPIC_ADMIN_KEY, which wins over any stored one.
    pub admin_key: Option<String>,
    /// Look for Claude Code's login in the macOS Keychain before the file.
    pub use_keychain: bool,
}

impl Env {
    pub fn system() -> Env {
        let home = dirs::home_dir().unwrap_or_default();
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        let claude_config_dir = var("CLAUDE_CONFIG_DIR");
        Env {
            api_base: var("KLAUDOMETER_API_BASE")
                .unwrap_or_else(|| "https://api.anthropic.com".into())
                .trim_end_matches('/')
                .to_string(),
            github_api: var("KLAUDOMETER_GITHUB_API")
                .unwrap_or_else(|| "https://api.github.com".into())
                .trim_end_matches('/')
                .to_string(),
            // Claude Code keys its Keychain entry to a custom config folder,
            // under a name we can't predict, so use the file for those.
            use_keychain: cfg!(target_os = "macos") && claude_config_dir.is_none(),
            claude_dir: claude_config_dir.map(PathBuf::from).unwrap_or_else(|| home.join(".claude")),
            cache_dir: dirs::cache_dir().unwrap_or_else(|| home.join(".cache")).join("klaudometer"),
            config_dir: dirs::config_dir().unwrap_or_else(|| home.join(".config")).join("klaudometer"),
            admin_key: var("ANTHROPIC_ADMIN_KEY"),
        }
    }

    pub fn usage_cache(&self) -> PathBuf {
        self.cache_dir.join("usage.json")
    }

    pub fn api_cache(&self) -> PathBuf {
        self.cache_dir.join("api.json")
    }

    pub fn claims_file(&self) -> PathBuf {
        self.cache_dir.join("notified.json")
    }

    pub fn lock_file(&self) -> PathBuf {
        self.cache_dir.join("fetch.lock")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("desktop.json")
    }

    pub fn update_file(&self) -> PathBuf {
        self.cache_dir.join("update.json")
    }

    pub fn admin_key_file(&self) -> PathBuf {
        self.config_dir.join("admin-key")
    }
}
