//! Klaudometer for macOS and Windows (it runs on Linux too): a menu bar or
//! tray icon drawn from the usage rings, a menu with the details, a card
//! window, a settings window, and notifications. The logic lives in
//! klaudometer-core; this file wires it to Tauri.

mod ring_icon;

use klaudometer_core::credentials::{self, KeyStatus};
use klaudometer_core::meters::{palette, Colors, THEMES};
use klaudometer_core::{self as core, claims, Env, Settings, Summary, Usage};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent, Wry,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

const TRAY_ID: &str = "klaudometer";
const CARD: &str = "card";
const SETTINGS: &str = "settings";
const USAGE_PAGE: &str = "https://claude.ai/settings/usage";
const ADMIN_KEYS_PAGE: &str = "https://platform.claude.com/settings/admin-keys";
/// How often countdowns, pace and alerts are re-checked between fetches.
const TICK: Duration = Duration::from_secs(20);
const ICON_SIZE: u32 = 64;

struct AppState {
    inner: Mutex<Inner>,
    /// Wakes the background thread early, for Refresh Now.
    wake: Sender<()>,
}

#[derive(Default)]
struct Inner {
    usage: Option<Usage>,
    summary: Option<Summary>,
    force: bool,
    /// Alerts this process already offered to the shared alert record.
    known_alerts: HashSet<String>,
    last_icon: Vec<u8>,
    last_label: String,
    last_rows: Vec<String>,
    /// When the card last hid itself on losing focus, so the tray click that
    /// caused it doesn't immediately reopen it.
    hidden_on_blur: Option<Instant>,
    /// The card opened above the tray icon, so it grows upwards.
    card_above: bool,
}

/// What the tray needs after an update, applied outside the lock.
struct TrayUpdate {
    icon: Option<Vec<u8>>,
    label: Option<String>,
    tooltip: String,
    menu: Option<Summary>,
}

pub fn run() {
    let env = Env::system();
    let (wake, woken) = channel();
    tauri::Builder::default()
        // Must come first: a second launch just opens the card (or settings) of the first.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| match settings_arg(&args) {
            Some(page) => show_settings(app, &page),
            None => toggle_card(app, None),
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .manage(AppState { inner: Mutex::new(Inner::default()), wake })
        .invoke_handler(tauri::generate_handler![
            get_summary,
            refresh_now,
            open_link,
            open_settings,
            hide_card,
            fit_card,
            get_settings,
            save_settings,
            set_start_at_login,
            admin_key_status,
            save_admin_key,
            remove_admin_key,
            test_notification,
        ])
        .setup(move |app| {
            // A menu bar app: no Dock icon.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            let first = core::summary(None, &Settings::load(&env.settings_file()), core::now());
            TrayIconBuilder::with_id(TRAY_ID)
                .icon(Image::new_owned(ring_icon::render(&first.rings, ICON_SIZE), ICON_SIZE, ICON_SIZE))
                .tooltip("Klaudometer")
                .menu(&build_menu(&handle, &first)?)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| on_menu(app, event.id.as_ref()))
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, position, .. } = event {
                        toggle_card(tray.app_handle(), Some(position));
                    }
                })
                .build(app)?;

            if let Some(card) = app.get_webview_window(CARD) {
                let card_handle = card.clone();
                let app_handle = handle.clone();
                card.on_window_event(move |event| match event {
                    // Behave like a menu bar popover: close when you click away.
                    WindowEvent::Focused(false) => {
                        let _ = card_handle.hide();
                        app_handle.state::<AppState>().inner.lock().unwrap().hidden_on_blur = Some(Instant::now());
                    }
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        let _ = card_handle.hide();
                    }
                    _ => {}
                });
            }

            if let Some(page) = settings_arg(&std::env::args().collect::<Vec<_>>()) {
                show_settings(&handle, &page);
            }
            std::thread::spawn(move || worker(handle, env, woken));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Klaudometer")
        .run(|_app, event| {
            // Hiding the card mustn't quit the app; only Quit (an explicit exit code) does.
            if let RunEvent::ExitRequested { code: None, api, .. } = event {
                api.prevent_exit();
            }
        });
}

/// The background loop: fetch when due (or when asked), then refresh the
/// tray, the card and alerts every tick.
fn worker(app: AppHandle, env: Env, woken: Receiver<()>) {
    let mut last_fetch = f64::MIN;
    let mut last_forced = 0.0;
    let mut last_sources = (false, false);
    loop {
        // Re-read each time: the settings window saves to the file and wakes us.
        let settings = Settings::load(&env.settings_file());
        let now = core::now();
        let state = app.state::<AppState>();
        let force = std::mem::take(&mut state.inner.lock().unwrap().force);
        // Fetch straight away when a meter that needs other data is switched on.
        let sources = (settings.wants_subscription(), settings.show_api);
        if force || now >= last_fetch + settings.refresh_seconds() || sources != last_sources {
            let usage = core::fetch(&env, &settings.request(force), now);
            last_fetch = now;
            last_sources = sources;
            state.inner.lock().unwrap().usage = Some(usage);
        }
        let rolled_over = update(&app, &env, &settings, now);
        // A window just reset: fetch its fresh numbers (at most every 2 minutes).
        if rolled_over && now - last_forced > 120.0 {
            last_forced = now;
            state.inner.lock().unwrap().force = true;
            continue;
        }
        let _ = woken.recv_timeout(TICK);
    }
}

/// Recompute the summary, then update the tray, the card and alerts.
/// Returns whether any window has rolled over since the last fetch.
fn update(app: &AppHandle, env: &Env, settings: &Settings, now: f64) -> bool {
    let state = app.state::<AppState>();
    let (summary, tray, alerts) = {
        let mut inner = state.inner.lock().unwrap();
        let summary = core::summary(inner.usage.as_ref(), settings, now);

        let icon = ring_icon::render(&summary.rings, ICON_SIZE);
        let icon = (icon != inner.last_icon).then(|| {
            inner.last_icon = icon.clone();
            icon
        });
        let label = (summary.label != inner.last_label).then(|| {
            inner.last_label = summary.label.clone();
            summary.label.clone()
        });
        let rows: Vec<String> = summary.rows.iter().map(|r| r.text.clone()).collect();
        let menu = (rows != inner.last_rows).then(|| {
            inner.last_rows = rows;
            summary.clone()
        });
        // Windows caps tray tooltips at about 127 characters, so keep it short.
        let full = Settings { label: "full".into(), ..settings.clone() };
        let tooltip = match core::meters::label_text(&summary.rings, &full) {
            text if text.is_empty() => "Klaudometer".to_string(),
            text => format!("Klaudometer\n{text}"),
        };

        let alerts = match &inner.usage {
            Some(usage) => core::alert_requests(&summary.meters, settings, usage, now)
                .into_iter()
                .filter(|a| inner.known_alerts.insert(a.id.clone()))
                .collect(),
            None => Vec::new(),
        };
        inner.summary = Some(summary.clone());
        (summary, TrayUpdate { icon, label, tooltip, menu }, alerts)
    };

    if let Some(tray_icon) = app.tray_by_id(TRAY_ID) {
        if let Some(icon) = tray.icon {
            let _ = tray_icon.set_icon(Some(Image::new_owned(icon, ICON_SIZE, ICON_SIZE)));
        }
        if let Some(label) = tray.label {
            // Shown beside the icon in the macOS menu bar and as the
            // AppIndicator label on Linux; Windows has no tray text.
            let _ = tray_icon.set_title(if label.is_empty() { None } else { Some(label) });
        }
        let _ = tray_icon.set_tooltip(Some(tray.tooltip));
        if let Some(summary) = tray.menu {
            if let Ok(menu) = build_menu(app, &summary) {
                let _ = tray_icon.set_menu(Some(menu));
            }
        }
    }
    let _ = app.emit("summary", &summary);

    // The shared record hands each alert to only one Klaudometer front end.
    if !alerts.is_empty() {
        let ids: Vec<String> = alerts.iter().map(|a| a.id.clone()).collect();
        let claimed = claims::claim(env, &ids, now);
        for alert in alerts.iter().filter(|a| claimed.contains(&a.id)) {
            let _ = app.notification().builder().title(&alert.title).body(&alert.body).show();
        }
    }
    summary.meters.iter().any(|m| m.reset)
}

fn build_menu(app: &AppHandle, summary: &Summary) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::new(app)?;
    for row in &summary.rows {
        // Headlines open the card; detail lines are greyed out.
        let item = MenuItem::with_id(app, format!("row:{}", row.key), &row.text, !row.detail, None::<&str>)?;
        menu.append(&item)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    for (id, text) in [("card", "Show Card"), ("refresh", "Refresh Now"), ("usage", "Open Claude Usage Page")] {
        menu.append(&MenuItem::with_id(app, id, text, true, None::<&str>)?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit Klaudometer", true, None::<&str>)?)?;
    Ok(menu)
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "refresh" => request_refresh(app),
        "usage" => {
            let _ = app.opener().open_url(USAGE_PAGE, None::<&str>);
        }
        "settings" => show_settings(app, ""),
        "quit" => app.exit(0),
        _ if id == "card" || id.starts_with("row:") => show_card(app, None),
        _ => {}
    }
}

fn request_refresh(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.inner.lock().unwrap().force = true;
    let _ = state.wake.send(());
}

/// `--settings` or `--settings=<page>` (general, appearance, spending or
/// notifications) on the command line opens the settings window.
fn settings_arg(args: &[String]) -> Option<String> {
    args.iter().find_map(|arg| match arg.as_str() {
        "--settings" => Some(String::new()),
        _ => arg.strip_prefix("--settings=").map(|page| page.chars().filter(char::is_ascii_lowercase).collect()),
    })
}

/// Open the settings window at `page` (or where it was), or bring it
/// forward if it's already open.
fn show_settings(app: &AppHandle, page: &str) {
    if let Some(window) = app.get_webview_window(SETTINGS) {
        if !page.is_empty() {
            let _ = window.eval(format!("location.hash = '{page}'"));
        }
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    // Build it off the event loop's thread: doing it there deadlocks on Windows.
    let (app, url) = (app.clone(), format!("settings.html#{page}"));
    tauri::async_runtime::spawn(async move {
        let built = WebviewWindowBuilder::new(&app, SETTINGS, WebviewUrl::App(url.into()))
            .title("Klaudometer Settings")
            .inner_size(660.0, 600.0)
            .min_inner_size(560.0, 440.0)
            .center()
            .build();
        let Ok(window) = built else { return };
        // A menu bar app has no Dock icon. Show one while settings are open, so
        // the window can be found with Command-Tab like any other.
        #[cfg(target_os = "macos")]
        {
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
            let handle = app.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::Destroyed = event {
                    let _ = handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
            });
        }
        let _ = window.set_focus();
    });
}

fn toggle_card(app: &AppHandle, cursor: Option<PhysicalPosition<f64>>) {
    let Some(card) = app.get_webview_window(CARD) else { return };
    let just_hidden = app
        .state::<AppState>()
        .inner
        .lock()
        .unwrap()
        .hidden_on_blur
        .is_some_and(|t| t.elapsed() < Duration::from_millis(300));
    if card.is_visible().unwrap_or(false) || just_hidden {
        let _ = card.hide();
    } else {
        show_card(app, cursor);
    }
}

fn show_card(app: &AppHandle, cursor: Option<PhysicalPosition<f64>>) {
    let Some(card) = app.get_webview_window(CARD) else { return };
    let above = match cursor {
        Some(cursor) => place_near(&card, cursor),
        None => {
            let _ = card.center();
            false
        }
    };
    app.state::<AppState>().inner.lock().unwrap().card_above = above;
    let _ = card.show();
    let _ = card.set_focus();
}

/// Put the card beside the tray icon that was clicked: below it when the
/// tray is at the top of the screen (macOS), above it when at the bottom
/// (Windows), kept on the clicked screen. Returns whether it went above.
fn place_near(card: &WebviewWindow, cursor: PhysicalPosition<f64>) -> bool {
    let Ok(size) = card.outer_size() else { return false };
    let (w, h) = (f64::from(size.width), f64::from(size.height));
    let monitors = card.available_monitors().unwrap_or_default();
    let monitor = monitors
        .iter()
        .find(|m| {
            let (p, s) = (m.position(), m.size());
            let (x, y) = (f64::from(p.x), f64::from(p.y));
            cursor.x >= x && cursor.x < x + f64::from(s.width) && cursor.y >= y && cursor.y < y + f64::from(s.height)
        })
        .or(monitors.first());
    let (mx, my, mw, mh) = monitor
        .map(|m| (f64::from(m.position().x), f64::from(m.position().y), f64::from(m.size().width), f64::from(m.size().height)))
        .unwrap_or((0.0, 0.0, 1920.0, 1080.0));
    let margin = 12.0;
    let x = (cursor.x - w / 2.0).min(mx + mw - w - margin).max(mx + margin);
    let above = cursor.y >= my + mh / 2.0;
    let y = if above { cursor.y - h - margin } else { cursor.y + margin };
    let y = y.min(my + mh - h).max(my);
    let _ = card.set_position(PhysicalPosition::new(x, y));
    above
}

// --- Commands for the card ---------------------------------------------------

#[tauri::command]
async fn get_summary(state: tauri::State<'_, AppState>) -> Result<Option<Summary>, ()> {
    Ok(state.inner.lock().unwrap().summary.clone())
}

#[tauri::command]
async fn refresh_now(app: AppHandle) -> Result<(), ()> {
    request_refresh(&app);
    Ok(())
}

/// Open one of Klaudometer's links in the browser. Only these two.
#[tauri::command]
async fn open_link(app: AppHandle, link: String) -> Result<(), ()> {
    let url = match link.as_str() {
        "usage" => USAGE_PAGE,
        "admin-keys" => ADMIN_KEYS_PAGE,
        _ => return Ok(()),
    };
    let _ = app.opener().open_url(url, None::<&str>);
    Ok(())
}

#[tauri::command]
async fn open_settings(app: AppHandle) -> Result<(), ()> {
    show_settings(&app, "");
    Ok(())
}

#[tauri::command]
async fn hide_card(app: AppHandle) -> Result<(), ()> {
    if let Some(card) = app.get_webview_window(CARD) {
        let _ = card.hide();
    }
    Ok(())
}

/// Size the card to its content, in CSS pixels. A card that opened above
/// the tray icon keeps its bottom edge where it was, so it never grows
/// under the taskbar.
#[tauri::command]
async fn fit_card(app: AppHandle, width: f64, height: f64) -> Result<(), ()> {
    let Some(card) = app.get_webview_window(CARD) else { return Ok(()) };
    let size = LogicalSize::new(width.clamp(280.0, 720.0).ceil(), height.clamp(120.0, 720.0).ceil());
    let before = card.outer_size().ok();
    let _ = card.set_size(size);
    let above = app.state::<AppState>().inner.lock().unwrap().card_above;
    if above && card.is_visible().unwrap_or(false) {
        if let (Some(before), Ok(position), Ok(scale)) = (before, card.outer_position(), card.scale_factor()) {
            let grew = (size.height * scale).round() as i32 - before.height as i32;
            let _ = card.set_position(PhysicalPosition::new(position.x, position.y - grew));
        }
    }
    Ok(())
}

// --- Commands for the settings window ----------------------------------------

#[derive(Serialize)]
struct Theme {
    id: &'static str,
    name: &'static str,
    colors: Colors,
}

#[derive(Serialize)]
struct SettingsView {
    settings: Settings,
    defaults: Settings,
    start_at_login: bool,
    /// "macos", "windows" or "linux".
    platform: &'static str,
    themes: Vec<Theme>,
    version: String,
}

#[tauri::command]
async fn get_settings(app: AppHandle) -> Result<SettingsView, ()> {
    let themes = THEMES
        .iter()
        .map(|&(id, name)| Theme { id, name, colors: palette(&Settings { theme: id.into(), ..Settings::default() }) })
        .collect();
    Ok(SettingsView {
        settings: Settings::load(&Env::system().settings_file()),
        defaults: Settings::default(),
        start_at_login: app.autolaunch().is_enabled().unwrap_or(false),
        platform: std::env::consts::OS,
        themes,
        version: app.package_info().version.to_string(),
    })
}

/// Save the settings and apply them at once. Returns them as saved, with
/// numbers brought into range.
#[tauri::command]
async fn save_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    let settings = settings.normalized();
    settings.save(&Env::system().settings_file()).map_err(|e| format!("Couldn't save your settings: {e}"))?;
    let _ = app.state::<AppState>().wake.send(());
    Ok(settings)
}

#[tauri::command]
async fn set_start_at_login(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let autolaunch = app.autolaunch();
    let changed = if enabled { autolaunch.enable() } else { autolaunch.disable() };
    changed.map_err(|e| format!("Couldn't change this: {e}"))?;
    Ok(autolaunch.is_enabled().unwrap_or(enabled))
}

/// Where the Admin API key is. Separate from get_settings, since the
/// Keychain may ask for permission first.
#[tauri::command]
async fn admin_key_status() -> Result<KeyStatus, ()> {
    Ok(credentials::admin_key_status(&Env::system()))
}

/// Store a pasted Admin API key, then fetch API spend with it straight away.
#[tauri::command]
async fn save_admin_key(app: AppHandle, key: String) -> Result<KeyStatus, String> {
    let env = Env::system();
    credentials::store_admin_key(&env, &key)?;
    request_refresh(&app);
    Ok(credentials::admin_key_status(&env))
}

#[tauri::command]
async fn remove_admin_key(app: AppHandle) -> Result<KeyStatus, String> {
    let env = Env::system();
    credentials::remove_admin_key(&env)?;
    request_refresh(&app);
    Ok(credentials::admin_key_status(&env))
}

#[tauri::command]
async fn test_notification(app: AppHandle) -> Result<(), String> {
    app.notification()
        .builder()
        .title("Klaudometer")
        .body("Notifications are working.")
        .show()
        .map_err(|e| format!("Couldn't send a notification: {e}"))
}
