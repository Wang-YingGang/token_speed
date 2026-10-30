mod settings;
mod taskbar;
use settings::*;

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::RwLock;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Position, Size, WebviewWindow,
    WindowEvent,
};

const SETTINGS_WIDTH: f64 = 344.0;
const SETTINGS_HEIGHT: f64 = 510.0;
const HISTORY_LIMIT: usize = 20;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageRecord {
    request_id: String,
    app_type: String,
    model: String,
    output_tps: f64,
    created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppUsage {
    app_type: String,
    latest: Option<UsageRecord>,
    history: Vec<UsageRecord>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageSnapshot {
    apps: Vec<AppUsage>,
    database_found: bool,
    database_path: String,
    message: Option<String>,
    updated_at: i64,
}

#[derive(Debug)]
struct RawUsage {
    request_id: String,
    app_type: String,
    model: String,
    output_tokens: i64,
    latency_ms: i64,
    first_token_ms: Option<i64>,
    duration_ms: Option<i64>,
    created_at: i64,
}

fn database_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CC_SWITCH_DB") {
        return PathBuf::from(path);
    }
    dirs::home_dir()
        .unwrap_or_default()
        .join(".cc-switch")
        .join("cc-switch.db")
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn normalize_app(app_type: &str) -> &str {
    match app_type {
        "claude" | "claude-desktop" | "claude_code" => "claude",
        "codex" => "codex",
        other => other,
    }
}

fn generation_ms(latency_ms: i64, first_token_ms: Option<i64>, duration_ms: Option<i64>) -> i64 {
    match first_token_ms {
        Some(ttft) if ttft >= 0 && latency_ms > ttft => latency_ms - ttft,
        _ => duration_ms
            .filter(|value| *value > 0)
            .unwrap_or(latency_ms)
            .max(0),
    }
}

impl RawUsage {
    fn into_record(self) -> UsageRecord {
        let app_type = normalize_app(&self.app_type).to_string();
        let generation_ms = generation_ms(self.latency_ms, self.first_token_ms, self.duration_ms);
        let output_tps = if generation_ms > 0 {
            self.output_tokens as f64 * 1000.0 / generation_ms as f64
        } else {
            0.0
        };

        UsageRecord {
            request_id: self.request_id,
            app_type,
            model: self.model,
            output_tps,
            created_at: self.created_at,
        }
    }
}

fn empty_snapshot(path: &Path, found: bool, message: Option<String>) -> UsageSnapshot {
    UsageSnapshot {
        apps: vec![
            AppUsage {
                app_type: "claude".to_string(),
                latest: None,
                history: vec![],
            },
            AppUsage {
                app_type: "codex".to_string(),
                latest: None,
                history: vec![],
            },
        ],
        database_found: found,
        database_path: path.display().to_string(),
        message,
        updated_at: now_ms(),
    }
}

fn query_usage(path: &Path) -> Result<UsageSnapshot, String> {
    if !path.exists() {
        return Ok(empty_snapshot(
            path,
            false,
            Some("未找到 CC Switch 数据库".to_string()),
        ));
    }

    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("无法只读打开数据库：{error}"))?;
    connection
        .busy_timeout(Duration::from_millis(300))
        .map_err(|error| error.to_string())?;

    let mut statement = connection
        .prepare(
            "SELECT request_id, app_type, model, output_tokens,
                    latency_ms, first_token_ms, duration_ms, created_at, source_rowid
             FROM (
                 SELECT request_id, app_type, model, output_tokens,
                        latency_ms, first_token_ms, duration_ms, created_at,
                        rowid AS source_rowid
                 FROM proxy_request_logs
                 WHERE app_type IN ('claude', 'claude-desktop', 'claude_code')
                   AND status_code >= 200 AND status_code < 300
                   AND output_tokens > 0
                 ORDER BY created_at DESC, rowid DESC
                 LIMIT 120
             )
             UNION ALL
             SELECT request_id, app_type, model, output_tokens,
                    latency_ms, first_token_ms, duration_ms, created_at, source_rowid
             FROM (
                 SELECT request_id, app_type, model, output_tokens,
                        latency_ms, first_token_ms, duration_ms, created_at,
                        rowid AS source_rowid
                 FROM proxy_request_logs
                 WHERE app_type = 'codex'
                   AND status_code >= 200 AND status_code < 300
                   AND output_tokens > 0
                 ORDER BY created_at DESC, rowid DESC
                 LIMIT 120
             )
             ORDER BY created_at DESC, source_rowid DESC",
        )
        .map_err(|error| format!("无法读取用量记录：{error}"))?;

    let rows = statement
        .query_map([], |row| {
            Ok(RawUsage {
                request_id: row.get(0)?,
                app_type: row.get(1)?,
                model: row.get(2)?,
                output_tokens: row.get(3)?,
                latency_ms: row.get(4)?,
                first_token_ms: row.get(5)?,
                duration_ms: row.get(6)?,
                created_at: row.get(7)?,
            })
        })
        .map_err(|error| error.to_string())?;

    let mut histories: HashMap<String, Vec<UsageRecord>> = HashMap::new();
    let mut seen: HashMap<String, HashSet<String>> = HashMap::new();

    for row in rows {
        let raw = row.map_err(|error| error.to_string())?;
        let app_type = normalize_app(&raw.app_type).to_string();
        let app_seen = seen.entry(app_type.clone()).or_default();
        if !app_seen.insert(raw.request_id.clone()) {
            continue;
        }

        let history = histories.entry(app_type).or_default();
        if history.len() < HISTORY_LIMIT {
            history.push(raw.into_record());
        }
    }

    let mut snapshot = empty_snapshot(path, true, None);
    for app in &mut snapshot.apps {
        if let Some(history) = histories.remove(&app.app_type) {
            app.latest = history.first().cloned();
            app.history = history;
        }
    }
    Ok(snapshot)
}

fn load_snapshot() -> UsageSnapshot {
    let path = database_path();
    query_usage(&path).unwrap_or_else(|error| empty_snapshot(&path, path.exists(), Some(error)))
}

#[tauri::command]
fn get_snapshot() -> UsageSnapshot {
    load_snapshot()
}

#[tauri::command]
fn get_monitor_geometry(app: AppHandle) -> Result<taskbar::MonitorGeometry, String> {
    let window = app.get_webview_window("main").ok_or("监控窗口不存在")?;
    taskbar::monitor_geometry(&window)
}

fn position_settings(window: &WebviewWindow, main: &WebviewWindow) -> tauri::Result<()> {
    window.set_size(Size::Logical(LogicalSize::new(
        SETTINGS_WIDTH,
        SETTINGS_HEIGHT,
    )))?;
    if let Some(monitor) = main.current_monitor()? {
        let area = monitor.work_area();
        let size = window.outer_size()?;
        let anchor = main.outer_position()?;
        let main_size = main.outer_size()?;
        let margin = (12.0 * monitor.scale_factor()) as i32;
        let min_x = area.position.x + margin;
        let min_y = area.position.y + margin;
        let max_x =
            (area.position.x + area.size.width as i32 - size.width as i32 - margin).max(min_x);
        let max_y =
            (area.position.y + area.size.height as i32 - size.height as i32 - margin).max(min_y);
        let x = anchor.x + main_size.width as i32 / 2 - size.width as i32 / 2;
        let above = anchor.y - size.height as i32 - margin;
        let y = if above >= min_y {
            above
        } else {
            anchor.y + main_size.height as i32 + margin
        };
        window.set_position(Position::Physical(PhysicalPosition::new(
            x.clamp(min_x, max_x),
            y.clamp(min_y, max_y),
        )))?;
    }
    Ok(())
}

#[tauri::command]
fn show_settings(app: AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("settings").ok_or("设置窗口不存在")?;
    let main = app.get_webview_window("main").ok_or("监控窗口不存在")?;
    position_settings(&window, &main).map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

#[tauri::command]
fn hide_settings(app: AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("settings").ok_or("设置窗口不存在")?;
    window.hide().map_err(|e| e.to_string())
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    if let Ok(settings) = app.state::<RwLock<TaskbarSettings>>().read() {
        let _ = persist_taskbar_settings(&settings);
    }
    app.exit(0);
}

#[tauri::command]
fn show_panel(window: WebviewWindow) -> Result<(), String> {
    let settings = window.state::<RwLock<TaskbarSettings>>();
    let settings = settings.read().map_err(|_| "无法读取设置".to_string())?;
    apply_window_settings(&window, &settings)?;
    window.show().map_err(|error| error.to_string())
}

#[cfg(windows)]
#[tauri::command]
fn get_autostart() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        .and_then(|key| key.get_value::<String, _>("TokenSpeed"))
        .is_ok()
}

#[cfg(not(windows))]
#[tauri::command]
fn get_autostart() -> bool {
    false
}

#[cfg(windows)]
#[tauri::command]
fn set_autostart(enabled: bool) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = current_user
        .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        .map_err(|error| error.to_string())?;

    if enabled {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        key.set_value("TokenSpeed", &format!("\"{}\"", executable.display()))
            .map_err(|error| error.to_string())
    } else {
        match key.delete_value("TokenSpeed") {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

#[cfg(not(windows))]
#[tauri::command]
fn set_autostart(_enabled: bool) -> Result<(), String> {
    Ok(())
}

fn is_usage_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            matches!(
                name,
                "cc-switch.db" | "cc-switch.db-wal" | "cc-switch.db-shm"
            )
        })
        .unwrap_or(false)
}

fn file_signature(path: &Path) -> (u64, u64) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return (0, 0);
    };
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    (metadata.len(), modified)
}

fn spawn_database_watcher(app: AppHandle) {
    thread::spawn(move || {
        let path = database_path();
        let _ = app.emit("usage-changed", load_snapshot());

        let (sender, receiver) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(
            move |result: notify::Result<notify::Event>| {
                let _ = sender.send(result);
            },
            Config::default(),
        );

        if let (Ok(watcher), Some(parent)) = (watcher.as_mut(), path.parent()) {
            let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
        }

        let mut last_signature = (
            file_signature(&path),
            file_signature(&path.with_extension("db-wal")),
        );
        let mut last_poll = Instant::now();

        loop {
            match receiver.recv_timeout(Duration::from_millis(400)) {
                Ok(Ok(event)) => {
                    if !event.paths.iter().any(|path| is_usage_file(path)) {
                        continue;
                    }
                    thread::sleep(Duration::from_millis(80));
                    while receiver.try_recv().is_ok() {}
                    let _ = app.emit("usage-changed", load_snapshot());
                    last_signature = (
                        file_signature(&path),
                        file_signature(&path.with_extension("db-wal")),
                    );
                    last_poll = Instant::now();
                }
                Ok(Err(_)) | Err(RecvTimeoutError::Disconnected) => {
                    thread::sleep(Duration::from_millis(500));
                }
                Err(RecvTimeoutError::Timeout) => {}
            }

            if last_poll.elapsed() >= Duration::from_secs(1) {
                // WebView timers are throttled for taskbar child windows.
                // Drive freshness expiry from native events even without DB changes.
                let _ = app.emit("activity-tick", now_ms());
                let signature = (
                    file_signature(&path),
                    file_signature(&path.with_extension("db-wal")),
                );
                if signature != last_signature {
                    let _ = app.emit("usage-changed", load_snapshot());
                    last_signature = signature;
                }
                last_poll = Instant::now();
            }
        }
    });
}

fn spawn_taskbar_watcher(window: WebviewWindow) {
    thread::spawn(move || {
        let mut last_saved = None;
        loop {
            thread::sleep(Duration::from_secs(1));
            let state = window.state::<RwLock<TaskbarSettings>>();
            let Ok(settings) = state.read() else {
                continue;
            };
            if window.is_visible().unwrap_or(false) && settings.window_mode == "taskbar" {
                let _ = taskbar::embed_taskbar(&window, settings.offset, settings.width());
            }
            if last_saved.as_ref() != Some(&*settings) {
                if persist_taskbar_settings(&settings).is_ok() {
                    last_saved = Some(settings.clone());
                }
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let taskbar_settings = load_taskbar_settings();
    tauri::Builder::default()
        .manage(RwLock::new(taskbar_settings))
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_monitor_geometry,
            get_taskbar_settings,
            set_monitor_settings,
            reset_monitor_position,
            start_monitor_drag,
            show_settings,
            set_display_mode,
            set_taskbar_offset,
            hide_settings,
            quit_app,
            show_panel,
            get_autostart,
            set_autostart
        ])
        .setup(|app| {
            let window = app.get_webview_window("main").expect("main window");
            {
                let state = app.state::<RwLock<TaskbarSettings>>();
                let mut settings = state.write().map_err(|_| "无法读取设置")?;
                if let Err(error) = apply_window_settings(&window, &settings) {
                    if settings.window_mode != "taskbar" {
                        return Err(error.into());
                    }
                    settings.window_mode = "floating".into();
                    apply_window_settings(&window, &settings)?;
                }
            }
            window.show()?;
            spawn_taskbar_watcher(window);

            let show_item = MenuItem::with_id(app, "show", "显示 / 隐藏", true, None::<&str>)?;
            let settings_item = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 Token Speed", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &settings_item, &quit_item])?;
            let mut tray = TrayIconBuilder::new()
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        let _ = show_settings(tray.app_handle().clone());
                    }
                })
                .tooltip("Token Speed — CC Switch 用量监控")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                                if let Some(settings_window) = app.get_webview_window("settings") {
                                    let _ = settings_window.hide();
                                }
                            } else {
                                let _ = show_panel(window);
                            }
                        }
                    }
                    "settings" => {
                        let _ = show_settings(app.clone());
                    }
                    "quit" => app.exit(0),
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            spawn_database_watcher(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "settings" && matches!(event, WindowEvent::Focused(false)) {
                let window = window.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(140));
                    // Native select popups belong to this window and must stay usable.
                    if taskbar::settings_lost_focus(&window) {
                        let _ = window.hide();
                    }
                });
            }
            if window.label() == "main" {
                if let WindowEvent::Moved(position) = event {
                    let state = window.state::<RwLock<TaskbarSettings>>();
                    if let Ok(mut settings) = state.try_write() {
                        if settings.window_mode == "floating" {
                            settings.floating_x = Some(position.x);
                            settings.floating_y = Some(position.y);
                        }
                    };
                }
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Token Speed");
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_DATABASE_ID: AtomicUsize = AtomicUsize::new(0);

    fn test_database() -> PathBuf {
        let id = TEST_DATABASE_ID.fetch_add(1, Ordering::Relaxed);
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-dbs");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("usage-{}-{id}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE proxy_request_logs (
                    request_id TEXT,
                    app_type TEXT,
                    model TEXT,
                    input_tokens INTEGER,
                    output_tokens INTEGER,
                    latency_ms INTEGER,
                    first_token_ms INTEGER,
                    duration_ms INTEGER,
                    status_code INTEGER,
                    is_streaming INTEGER,
                    data_source TEXT,
                    created_at INTEGER
                )",
            )
            .unwrap();
        path
    }

    fn insert_usage(
        path: &Path,
        request_id: &str,
        app_type: &str,
        output_tokens: i64,
        status_code: i64,
        created_at: i64,
    ) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute(
                "INSERT INTO proxy_request_logs (
                    request_id, app_type, model, output_tokens,
                    latency_ms, first_token_ms, duration_ms, status_code,
                    is_streaming, data_source, created_at
                 ) VALUES (?1, ?2, 'test-model', ?3, 10000, 2500, 9000, ?4, 1, 'test', ?5)",
                params![request_id, app_type, output_tokens, status_code, created_at],
            )
            .unwrap();
    }

    #[test]
    fn generation_speed_excludes_first_token_wait() {
        assert_eq!(generation_ms(10_000, Some(2_500), None), 7_500);
    }

    #[test]
    fn generation_speed_falls_back_to_duration_then_latency() {
        assert_eq!(generation_ms(10_000, None, Some(8_000)), 8_000);
        assert_eq!(generation_ms(10_000, Some(-1), Some(8_000)), 8_000);
        assert_eq!(generation_ms(10_000, None, None), 10_000);
        assert_eq!(generation_ms(10_000, Some(10_000), None), 10_000);
    }

    #[test]
    fn valid_first_token_takes_precedence_over_duration() {
        assert_eq!(generation_ms(10_000, Some(2_000), Some(5_000)), 8_000);
    }

    #[test]
    fn output_speed_uses_only_generation_time() {
        let record = RawUsage {
            request_id: "request".to_string(),
            app_type: "claude".to_string(),
            model: "model".to_string(),
            output_tokens: 300,
            latency_ms: 10_000,
            first_token_ms: Some(2_500),
            duration_ms: None,
            created_at: 1,
        }
        .into_record();
        assert!((record.output_tps - 40.0).abs() < f64::EPSILON);
    }

    #[test]
    fn claude_variants_share_one_display_slot() {
        assert_eq!(normalize_app("claude"), "claude");
        assert_eq!(normalize_app("claude-desktop"), "claude");
        assert_eq!(normalize_app("claude_code"), "claude");
        assert_eq!(normalize_app("codex"), "codex");
    }

    #[test]
    fn query_deduplicates_requests_and_normalizes_claude() {
        let path = test_database();
        insert_usage(&path, "same-request", "claude", 100, 200, 100);
        insert_usage(&path, "same-request", "claude_code", 200, 200, 100);
        insert_usage(&path, "codex-request", "codex", 300, 200, 101);
        let snapshot = query_usage(&path).unwrap();
        let claude = &snapshot.apps[0];
        assert_eq!(claude.history.len(), 1);
        assert!((claude.latest.as_ref().unwrap().output_tps - 200.0 / 7.5).abs() < 0.0001);
        assert_eq!(claude.latest.as_ref().unwrap().app_type, "claude");
        assert_eq!(snapshot.apps[1].history.len(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn query_keeps_only_successful_output_requests() {
        let path = test_database();
        insert_usage(&path, "failed", "claude", 100, 500, 300);
        insert_usage(&path, "empty", "claude", 0, 200, 200);
        insert_usage(&path, "success", "claude", 100, 200, 100);
        let snapshot = query_usage(&path).unwrap();
        assert_eq!(snapshot.apps[0].history.len(), 1);
        assert_eq!(
            snapshot.apps[0].latest.as_ref().unwrap().request_id,
            "success"
        );
        let _ = std::fs::remove_file(path);
    }
}
