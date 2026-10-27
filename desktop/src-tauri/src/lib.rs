//! Klaudometer for macOS and Windows (it runs on Linux too): a menu bar or
//! tray icon drawn from the usage rings, a menu with the details, a card
//! window, and notifications. The logic lives in klaudometer-core; this file
//! wires it to Tauri.

mod ring_icon;

use klaudometer_core::{self as core, claims, Env, Settings, Summary, Usage};
use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, RunEvent, WebviewWindow, WindowEvent, Wry};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

const TRAY_ID: &str = "klaudometer";
const CARD: &str = "card";
const USAGE_PAGE: &str = "https://claude.ai/settings/usage";
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
        // Must come first: a second launch just opens the card of the first.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| toggle_card(app, None)))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState { inner: Mutex::new(Inner::default()), wake })
        .invoke_handler(tauri::generate_handler![get_summary, refresh_now, open_usage_page, open_settings, hide_card])
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
    let mut next_fetch = 0.0;
    let mut last_forced = 0.0;
    loop {
        // Re-read each tick so edits to the settings file apply quickly.
        let settings = Settings::load(&env.settings_file());
        let now = core::now();
        let state = app.state::<AppState>();
        let force = std::mem::take(&mut state.inner.lock().unwrap().force);
        if force || now >= next_fetch {
            let usage = core::fetch(&env, &settings.request(force), now);
            next_fetch = now + settings.refresh_seconds();
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
    menu.append(&MenuItem::with_id(app, "settings", "Edit Settings…", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit Klaudometer", true, None::<&str>)?)?;
    Ok(menu)
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "refresh" => request_refresh(app),
        "usage" => {
            let _ = app.opener().open_url(USAGE_PAGE, None::<&str>);
        }
        "settings" => open_settings_file(app),
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

fn open_settings_file(app: &AppHandle) {
    let env = Env::system();
    let path = env.settings_file();
    if !path.exists() {
        let _ = Settings::default().save(&path);
    }
    let _ = app.opener().open_path(path.to_string_lossy(), None::<&str>);
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
    match cursor {
        Some(cursor) => place_near(&card, cursor),
        None => {
            let _ = card.center();
        }
    }
    let _ = card.show();
    let _ = card.set_focus();
}

/// Put the card beside the tray icon that was clicked: below it when the
/// tray is at the top of the screen (macOS), above it when at the bottom
/// (Windows), kept on the clicked screen.
fn place_near(card: &WebviewWindow, cursor: PhysicalPosition<f64>) {
    let Ok(size) = card.outer_size() else { return };
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
    let y = if cursor.y < my + mh / 2.0 { cursor.y + margin } else { cursor.y - h - margin };
    let y = y.min(my + mh - h).max(my);
    let _ = card.set_position(PhysicalPosition::new(x, y));
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

#[tauri::command]
async fn open_usage_page(app: AppHandle) -> Result<(), ()> {
    let _ = app.opener().open_url(USAGE_PAGE, None::<&str>);
    Ok(())
}

#[tauri::command]
async fn open_settings(app: AppHandle) -> Result<(), ()> {
    open_settings_file(&app);
    Ok(())
}

#[tauri::command]
async fn hide_card(app: AppHandle) -> Result<(), ()> {
    if let Some(card) = app.get_webview_window(CARD) {
        let _ = card.hide();
    }
    Ok(())
}
