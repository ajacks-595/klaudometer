# Klaudometer desktop app (macOS, Windows, Linux)

A [Tauri 2](https://tauri.app) app that puts Klaudometer in the macOS menu bar or the Windows system
tray: an icon drawn from the usage rings, a menu with the details, a card window, and notifications.
It runs on Linux too. There the [Plasma widget](../README.md#plasma-widget) and the
[tray icon](../README.md#tray-icon-gnome-xfce-cinnamon-and-others) remain the main options.

**Status:** first released in 1.1.0. Install it from the [Releases page](https://github.com/ajacks-595/klaudometer/releases),
as the [main README](../README.md#macos-and-windows) explains.

## Layout

| Path | What it is |
|---|---|
| `core/` | `klaudometer-core`: all the logic, with no GUI. It fetches usage and API spend, reads credentials, and handles the shared cache, alert claims, meters, pace and formatting. |
| `src-tauri/` | The app: tray or menu bar icon, menu, card window, notifications, and a background refresh thread. |
| `ui/` | The card window: plain HTML, CSS and JS, with no build step. |
| `src-tauri/icons/` | App icons. Regenerate them with `python3 scripts/make_icons.py src-tauri/icons` (needs ImageMagick). Tauri only loads 8-bit RGBA PNGs, and `--check` verifies that. |

The core is a port of the Linux helper (`package/contents/code/fetch_usage.py`) and the tray icon's
logic. It uses the same endpoints and the same cache and alert files, so on Linux all of Klaudometer's
front ends share one cache, and an alert is only sent once.

## Settings

**Settings…** in the tray menu, or the link on the card, opens the settings window. Changes are
saved as you make them, and the icon and card update straight away.

| Page | What's on it |
|---|---|
| General | Which meters to show, used or left, pace, the text beside the icon (macOS and Linux), how often to check, start at login, and the daily update check. |
| Appearance | Theme, single colour overrides, and the warning and critical levels. |
| Spending | Claude API spend, the monthly budget, and saving or removing the Admin API key. |
| Notifications | Which alerts to send, and a test notification. |

A pasted Admin API key goes straight into the Keychain, Credential Manager or (on Linux) your wallet
through `secret-tool`. On Linux without a wallet, it goes into a key file only you can read. The
window only ever learns where the key is, never the key itself.

`klaudometer --settings` opens the settings window, and `--settings=spending` (or `general`,
`appearance`, `notifications`) opens it on that page. If Klaudometer is already running, the running
copy opens it. For scripted setups, `klaudometer --store-admin-key` saves a key piped to it the same
way the settings window does.

On macOS, save the key from Klaudometer rather than with the `security` tool: a Keychain item the
tool made makes macOS ask before Klaudometer can read it.

## Where things live

| | macOS | Windows | Linux |
|---|---|---|---|
| Claude Code login | Keychain (`Claude Code-credentials`), falling back to `~/.claude/.credentials.json` | `%USERPROFILE%\.claude\.credentials.json` | `~/.claude/.credentials.json` |
| Admin API key | Keychain (service `klaudometer`, account `admin`) | Credential Manager (`klaudometer` / `admin`) | Secret Service (`service=klaudometer key=admin`) |
| Admin key fallback file | `~/Library/Application Support/klaudometer/admin-key` | `%APPDATA%\klaudometer\admin-key` | `~/.config/klaudometer/admin-key` |
| Settings | `~/Library/Application Support/klaudometer/desktop.json` | `%APPDATA%\klaudometer\desktop.json` | `~/.config/klaudometer/desktop.json` |
| Cache and alert record | `~/Library/Caches/klaudometer/` | `%LOCALAPPDATA%\klaudometer\` | `~/.cache/klaudometer/` |

`$ANTHROPIC_ADMIN_KEY` overrides the stored key, and `$CLAUDE_CONFIG_DIR` changes where Claude Code's
login is read from. For testing, `$KLAUDOMETER_API_BASE` points the Anthropic requests at another
server, and `$KLAUDOMETER_GITHUB_API` does the same for the update check.

## Builds

The [Desktop app workflow](../.github/workflows/desktop.yml) builds a universal macOS `.dmg` and
Windows `.msi` and `.exe` installers. Each build is smoke-tested on a real Mac and Windows machine
against a mock of Anthropic's API (`.github/scripts/`), and the screenshots it takes are uploaded
with the installers. It runs on pull requests, and on pushes through the Release workflow.

The builds aren't signed yet, so:

- **macOS** blocks the app the first time. Open it from **System Settings › Privacy & Security ›
  Open Anyway**.
- **macOS** may ask once after each update whether Klaudometer can use its Admin API key in the
  Keychain, because an unsigned app looks new to macOS each time. Choose **Always Allow**.
- **Windows** SmartScreen may warn about an unknown publisher. Choose **More info › Run anyway**.
- **Windows 11** puts new tray icons in the hidden icons area (the **^** by the clock). Drag
  Klaudometer's icon onto the taskbar, or turn it on in **Settings › Personalization › Taskbar ›
  Other system tray icons**.

## Develop

You need Rust and, on Linux, the WebKitGTK and AppIndicator development packages. On Fedora:

```bash
sudo dnf install webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel
```

```bash
cargo test                      # core unit tests, mock-API tests and the tray icon test
cargo run -p klaudometer        # run the app
```

Running the app a second time opens the first copy's card. On Linux the tray icon only opens its
menu, so use **Show Card** there.

## Releasing

Klaudometer has one version number for everything, and the [Release
workflow](../.github/workflows/release.yml) keeps a draft GitHub release for it up to date.

1. Bump the version in `desktop/Cargo.toml`, `desktop/src-tauri/tauri.conf.json`,
   `package/metadata.json` and the `USER_AGENT` in `package/contents/code/fetch_usage.py`.
   `python3 .github/scripts/check_versions.py` checks they match, and CI runs it too.
2. Write the release notes in `docs/release-notes/v<version>.md`.
3. Push to main. The workflow builds and smoke-tests the app, builds the `.plasmoid`, and creates
   or updates the draft release `v<version>` with every download and `SHA256SUMS.txt`.
4. Check the draft on GitHub and publish it. Publishing creates the tag.

The workflow never publishes. Once a version is published, later pushes leave it alone until the
version is bumped.
