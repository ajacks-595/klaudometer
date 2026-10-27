# Klaudometer desktop app (macOS, Windows, Linux)

A [Tauri 2](https://tauri.app) app that puts Klaudometer in the macOS menu bar or the Windows system
tray: an icon drawn from the usage rings, a menu with the details, a card window, and notifications.
It runs on Linux too. There the [Plasma widget](../README.md#plasma-widget) and the
[tray icon](../README.md#tray-icon-gnome-xfce-cinnamon-and-others) remain the main options.

**Status: in development.** It isn't released yet.

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

## Where things live

| | macOS | Windows | Linux |
|---|---|---|---|
| Claude Code login | Keychain (`Claude Code-credentials`), falling back to `~/.claude/.credentials.json` | `%USERPROFILE%\.claude\.credentials.json` | `~/.claude/.credentials.json` |
| Admin API key | Keychain (service `klaudometer`, account `admin`) | Credential Manager (`klaudometer` / `admin`) | Secret Service (`service=klaudometer key=admin`) |
| Admin key fallback file | `~/Library/Application Support/klaudometer/admin-key` | `%APPDATA%\klaudometer\admin-key` | `~/.config/klaudometer/admin-key` |
| Settings | `~/Library/Application Support/klaudometer/desktop.json` | `%APPDATA%\klaudometer\desktop.json` | `~/.config/klaudometer/desktop.json` |
| Cache and alert record | `~/Library/Caches/klaudometer/` | `%LOCALAPPDATA%\klaudometer\` | `~/.cache/klaudometer/` |

`$ANTHROPIC_ADMIN_KEY` overrides the stored key, `$CLAUDE_CONFIG_DIR` changes where Claude Code's
login is read from, and `$KLAUDOMETER_API_BASE` points everything at another server, for testing.

## Builds

The [Desktop app workflow](../.github/workflows/desktop.yml) builds a universal macOS `.dmg` and
Windows `.msi` and `.exe` installers on every push. Each build is smoke-tested on a real Mac and
Windows machine against a mock of Anthropic's API (`.github/scripts/`), and the screenshots it takes
are uploaded with the installers.

The builds aren't signed yet, so:

- **macOS** blocks the app the first time. Open it from **System Settings › Privacy & Security ›
  Open Anyway**.
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
