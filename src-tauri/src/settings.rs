use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::RwLock};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Position, WebviewWindow};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct TaskbarSettings {
    pub display_mode: String,
    pub offset: i32,
    pub window_mode: String,
    pub monitor_target: String,
    pub horizontal_anchor: String,
    pub show_chart: bool,
    pub font_size: u32,
    pub opacity: u32,
    pub always_on_top: bool,
    pub position_locked: bool,
    pub idle_after_seconds: u32,
    pub chart_width: u32,
    pub show_session_output: bool,
    pub session_output_width: u32,
    pub theme: String,
    pub floating_x: Option<i32>,
    pub floating_y: Option<i32>,
}

impl Default for TaskbarSettings {
    fn default() -> Self {
        Self {
            display_mode: "both".into(),
            offset: 0,
            window_mode: "taskbar".into(),
            monitor_target: "primary".into(),
            horizontal_anchor: "right".into(),
            show_chart: true,
            font_size: 11,
            opacity: 92,
            always_on_top: true,
            position_locked: false,
            idle_after_seconds: 15,
            chart_width: 40,
            show_session_output: true,
            session_output_width: 220,
            theme: "dark".into(),
            floating_x: None,
            floating_y: None,
        }
    }
}

impl TaskbarSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.display_mode.as_str(), "both" | "claude" | "codex")
            || !matches!(self.window_mode.as_str(), "taskbar" | "floating")
            || !matches!(
                self.monitor_target.as_str(),
                "primary" | "secondary" | "all"
            )
            || !matches!(self.horizontal_anchor.as_str(), "left" | "right")
            || !matches!(self.theme.as_str(), "dark" | "light")
            || !(10..=16).contains(&self.font_size)
            || !(30..=100).contains(&self.opacity)
            || !(-5_000..=5_000).contains(&self.offset)
            || !matches!(self.idle_after_seconds, 15 | 30 | 60)
            || !(24..=80).contains(&self.chart_width)
            || !(120..=360).contains(&self.session_output_width)
        {
            return Err("设置值超出支持范围".into());
        }
        Ok(())
    }

    pub fn width(&self) -> f64 {
        let text = if self.display_mode == "both" {
            88.0
        } else {
            50.0
        } * self.font_size as f64
            / 11.0;
        text + if self.show_session_output {
            self.session_output_width as f64 + 8.0
        } else {
            0.0
        } + if self.show_chart {
            self.chart_width as f64
        } else {
            0.0
        } + if self.window_mode == "floating" {
            46.0
        } else {
            14.0
        }
    }

    pub fn height(&self) -> f64 {
        if self.display_mode == "both" {
            (self.font_size * 2 + 22) as f64
        } else {
            (self.font_size + 23) as f64
        }
    }
}

fn settings_path() -> PathBuf {
    std::env::var_os("TOKEN_SPEED_SETTINGS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::config_dir()
                .unwrap_or_default()
                .join("TokenSpeed")
                .join("settings.json")
        })
}

pub fn load_taskbar_settings() -> TaskbarSettings {
    if let Ok(content) = std::fs::read_to_string(settings_path()) {
        if let Ok(settings) = serde_json::from_str::<TaskbarSettings>(&content) {
            if settings.validate().is_ok() {
                return settings;
            }
        }
    }
    let mut settings = TaskbarSettings::default();
    #[cfg(windows)]
    if let Ok(key) =
        winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey("Software\\TokenSpeed")
    {
        settings.display_mode = key
            .get_value("DisplayMode")
            .unwrap_or(settings.display_mode);
        settings.offset = key
            .get_value::<String, _>("Offset")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
    }
    if settings.validate().is_ok() {
        settings
    } else {
        TaskbarSettings::default()
    }
}

pub fn persist_taskbar_settings(settings: &TaskbarSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, content).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, &path).map_err(|e| e.to_string())
}

fn clamp_floating_position(
    target: PhysicalPosition<i32>,
    screen_position: PhysicalPosition<i32>,
    screen_size: tauri::PhysicalSize<u32>,
    window_size: tauri::PhysicalSize<u32>,
) -> PhysicalPosition<i32> {
    let max_x = screen_position.x + screen_size.width.saturating_sub(window_size.width) as i32;
    let max_y = screen_position.y + screen_size.height.saturating_sub(window_size.height) as i32;
    PhysicalPosition::new(
        target.x.clamp(screen_position.x, max_x),
        target.y.clamp(screen_position.y, max_y),
    )
}

pub fn remember_floating_position(window: &WebviewWindow, settings: &mut TaskbarSettings) {
    if settings.window_mode == "floating" {
        if let Ok(position) = window.outer_position() {
            settings.floating_x = Some(position.x);
            settings.floating_y = Some(position.y);
        }
    }
}

fn same_monitor(left: &tauri::Monitor, right: &tauri::Monitor) -> bool {
    left.position() == right.position() && left.size() == right.size()
}

pub fn has_secondary_monitor(window: &WebviewWindow) -> bool {
    let Ok(primary) = window.primary_monitor() else {
        return false;
    };
    let Ok(monitors) = window.available_monitors() else {
        return false;
    };
    monitors.iter().any(|monitor| {
        primary
            .as_ref()
            .map(|primary| !same_monitor(primary, monitor))
            .unwrap_or(true)
    })
}

fn choose_monitor(
    window: &WebviewWindow,
    secondary: bool,
) -> tauri::Result<Option<tauri::Monitor>> {
    let primary = window.primary_monitor()?;
    let monitors = window.available_monitors()?;
    if secondary {
        let fallback = primary.clone().or_else(|| monitors.first().cloned());
        return Ok(monitors
            .into_iter()
            .find(|monitor| {
                primary
                    .as_ref()
                    .map(|primary| !same_monitor(primary, monitor))
                    .unwrap_or(true)
            })
            .or(fallback));
    }
    Ok(primary.or_else(|| monitors.first().cloned()))
}

fn apply_window_settings_on(
    window: &WebviewWindow,
    settings: &TaskbarSettings,
    secondary: bool,
) -> Result<(), String> {
    if settings.window_mode == "taskbar" {
        window.set_always_on_top(false).map_err(|e| e.to_string())?;
        crate::taskbar::embed_taskbar(
            window,
            settings.offset,
            settings.width(),
            settings.horizontal_anchor == "right",
            secondary,
        )
    } else {
        crate::taskbar::detach_taskbar(window)?;
        window
            .set_size(tauri::Size::Logical(tauri::LogicalSize::new(
                settings.width(),
                settings.height(),
            )))
            .map_err(|e| e.to_string())?;
        window
            .set_always_on_top(settings.always_on_top)
            .map_err(|e| e.to_string())?;
        let size = window.outer_size().map_err(|e| e.to_string())?;
        let target = settings.floating_x.zip(settings.floating_y);
        let monitor = choose_monitor(window, secondary)
            .map_err(|e| e.to_string())?
            .or_else(|| window.current_monitor().ok().flatten());
        if let Some(monitor) = monitor {
            // New windows start above the taskbar. Saved positions may overlap
            // the taskbar, so only the full screen bounds constrain restoration.
            let a = monitor.work_area();
            let default_x = a.position.x + a.size.width.saturating_sub(size.width + 16) as i32;
            let default_y = a.position.y + a.size.height.saturating_sub(size.height + 16) as i32;
            let (x, y) = target.unwrap_or((default_x, default_y));
            let position = clamp_floating_position(
                PhysicalPosition::new(x, y),
                *monitor.position(),
                *monitor.size(),
                size,
            );
            window
                .set_position(Position::Physical(position))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

pub fn apply_window_settings(
    window: &WebviewWindow,
    settings: &TaskbarSettings,
) -> Result<(), String> {
    apply_window_settings_on(window, settings, settings.monitor_target == "secondary")
}

pub fn sync_secondary_window(app: &AppHandle, settings: &TaskbarSettings) -> Result<(), String> {
    let Some(window) = app.get_webview_window("monitor-secondary") else {
        return Ok(());
    };
    let main_visible = app
        .get_webview_window("main")
        .and_then(|main| main.is_visible().ok())
        .unwrap_or(false);
    let should_show =
        settings.monitor_target == "all" && main_visible && has_secondary_monitor(&window);
    if !should_show {
        if window.is_visible().unwrap_or(false) {
            window.hide().map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    apply_window_settings_on(&window, settings, true)?;
    window.show().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_taskbar_settings(
    state: tauri::State<'_, RwLock<TaskbarSettings>>,
) -> Result<TaskbarSettings, String> {
    state
        .read()
        .map(|s| s.clone())
        .map_err(|_| "无法读取设置".into())
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsPatch {
    display_mode: Option<String>,
    window_mode: Option<String>,
    monitor_target: Option<String>,
    horizontal_anchor: Option<String>,
    show_chart: Option<bool>,
    font_size: Option<u32>,
    opacity: Option<u32>,
    always_on_top: Option<bool>,
    position_locked: Option<bool>,
    theme: Option<String>,
    idle_after_seconds: Option<u32>,
    chart_width: Option<u32>,
    show_session_output: Option<bool>,
    session_output_width: Option<u32>,
    offset: Option<i32>,
}

#[tauri::command]
pub fn set_monitor_settings(
    app: AppHandle,
    state: tauri::State<'_, RwLock<TaskbarSettings>>,
    patch: SettingsPatch,
) -> Result<TaskbarSettings, String> {
    let mut current = state.write().map_err(|_| "无法更新设置".to_string())?;
    let mut next = current.clone();
    macro_rules! apply { ($($field:ident),*) => { $(if let Some(value) = patch.$field { next.$field = value; })* }; }
    apply!(
        display_mode,
        window_mode,
        monitor_target,
        horizontal_anchor,
        show_chart,
        font_size,
        opacity,
        always_on_top,
        position_locked,
        theme,
        idle_after_seconds,
        chart_width,
        show_session_output,
        session_output_width,
        offset
    );
    next.validate()?;
    let window = app.get_webview_window("main").ok_or("监控窗口不存在")?;
    if current.monitor_target != next.monitor_target
        || current.horizontal_anchor != next.horizontal_anchor
    {
        next.offset = 0;
        next.floating_x = None;
        next.floating_y = None;
    }
    // A native move event may still be queued when a settings change arrives.
    // Read the current position before resizing instead of restoring stale data.
    if current.window_mode == "floating" {
        if let Ok(position) = window.outer_position() {
            next.floating_x = Some(position.x);
            next.floating_y = Some(position.y);
        }
    }
    if let Err(error) = apply_window_settings(&window, &next).and_then(|_| {
        remember_floating_position(&window, &mut next);
        sync_secondary_window(&app, &next).and_then(|_| persist_taskbar_settings(&next))
    }) {
        let _ = apply_window_settings(&window, &current);
        let _ = sync_secondary_window(&app, &current);
        return Err(error);
    }
    *current = next.clone();
    app.emit("taskbar-settings", &next)
        .map_err(|e| e.to_string())?;
    Ok(next)
}

#[tauri::command]
pub fn set_display_mode(
    app: AppHandle,
    state: tauri::State<'_, RwLock<TaskbarSettings>>,
    display_mode: String,
) -> Result<TaskbarSettings, String> {
    set_monitor_settings(
        app,
        state,
        SettingsPatch {
            display_mode: Some(display_mode),
            ..Default::default()
        },
    )
}

#[tauri::command]
pub fn set_taskbar_offset(
    app: AppHandle,
    state: tauri::State<'_, RwLock<TaskbarSettings>>,
    offset: i32,
    persist: bool,
) -> Result<TaskbarSettings, String> {
    let mut current = state.write().map_err(|_| "无法更新位置".to_string())?;
    let mut next = current.clone();
    next.offset = offset.clamp(-5_000, 5_000);
    if next.window_mode == "taskbar" {
        if let Some(window) = app.get_webview_window("main") {
            apply_window_settings(&window, &next)?;
        }
    }
    sync_secondary_window(&app, &next)?;
    if persist {
        persist_taskbar_settings(&next)?;
    }
    *current = next.clone();
    app.emit("taskbar-settings", &next)
        .map_err(|e| e.to_string())?;
    Ok(next)
}

#[tauri::command]
pub fn reset_monitor_position(
    app: AppHandle,
    state: tauri::State<'_, RwLock<TaskbarSettings>>,
) -> Result<TaskbarSettings, String> {
    let mut current = state.write().map_err(|_| "无法复位位置".to_string())?;
    let mut next = current.clone();
    next.offset = 0;
    next.floating_x = None;
    next.floating_y = None;
    if let Some(window) = app.get_webview_window("main") {
        apply_window_settings(&window, &next)?;
        remember_floating_position(&window, &mut next);
    }
    sync_secondary_window(&app, &next)?;
    persist_taskbar_settings(&next)?;
    *current = next.clone();
    app.emit("taskbar-settings", &next)
        .map_err(|e| e.to_string())?;
    Ok(next)
}

#[tauri::command]
pub fn start_monitor_drag(
    app: AppHandle,
    state: tauri::State<'_, RwLock<TaskbarSettings>>,
) -> Result<(), String> {
    let settings = state.read().map_err(|_| "无法读取设置".to_string())?;
    if settings.position_locked || settings.window_mode != "floating" {
        return Ok(());
    }
    if let Some(window) = app.get_webview_window("main") {
        window.start_dragging().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_settings_receive_new_defaults() {
        let s: TaskbarSettings =
            serde_json::from_str(r#"{"displayMode":"codex","offset":70}"#).unwrap();
        assert_eq!(s.display_mode, "codex");
        assert_eq!(s.window_mode, "taskbar");
        assert!(s.show_chart);
        assert!(s.show_session_output);
        assert_eq!(s.session_output_width, 220);
        s.validate().unwrap();
    }
    #[test]
    fn invalid_settings_are_rejected() {
        let mut s = TaskbarSettings::default();
        s.font_size = 99;
        assert!(s.validate().is_err());
        s.font_size = 11;
        s.window_mode = "unknown".into();
        assert!(s.validate().is_err());
    }
    #[test]
    fn single_app_does_not_reserve_name_space() {
        let mut s = TaskbarSettings::default();
        s.show_session_output = false;
        assert_eq!(s.width(), 142.0);
        s.display_mode = "codex".into();
        assert_eq!(s.width(), 104.0);
        s.display_mode = "claude".into();
        assert_eq!(s.width(), 104.0);
    }
    #[test]
    fn inline_output_width_is_reserved_and_validated() {
        let mut s = TaskbarSettings::default();
        assert_eq!(s.width(), 370.0);
        s.show_session_output = false;
        assert_eq!(s.width(), 142.0);
        s.session_output_width = 361;
        assert!(s.validate().is_err());
        s.session_output_width = 119;
        assert!(s.validate().is_err());
    }
    #[test]
    fn floating_sizes_fit_each_configuration() {
        let mut s = TaskbarSettings::default();
        let chart_width = s.width();
        s.show_chart = false;
        assert_eq!(chart_width - s.width(), 40.0);
        s.window_mode = "floating".into();
        assert!(s.width() >= 134.0);
        s.font_size = 16;
        assert!(s.height() >= 54.0);
    }
    #[test]
    fn floating_positions_can_overlap_each_taskbar_edge() {
        let screen = PhysicalPosition::new(0, 0);
        let size = tauri::PhysicalSize::new(1920, 1080);
        let window = tauri::PhysicalSize::new(200, 34);
        for (x, y) in [(400, 1040), (400, 0), (0, 400), (1720, 400)] {
            let target = PhysicalPosition::new(x, y);
            assert_eq!(
                clamp_floating_position(target, screen, size, window),
                target
            );
        }
    }
    #[test]
    fn floating_positions_are_clamped_only_at_full_screen_edges() {
        let position = clamp_floating_position(
            PhysicalPosition::new(1900, 1070),
            PhysicalPosition::new(0, 0),
            tauri::PhysicalSize::new(1920, 1080),
            tauri::PhysicalSize::new(200, 34),
        );
        assert_eq!(position, PhysicalPosition::new(1720, 1046));
        let negative_monitor = clamp_floating_position(
            PhysicalPosition::new(-1500, 1040),
            PhysicalPosition::new(-1920, 0),
            tauri::PhysicalSize::new(1920, 1080),
            tauri::PhysicalSize::new(200, 34),
        );
        assert_eq!(negative_monitor, PhysicalPosition::new(-1500, 1040));
        let oversized = clamp_floating_position(
            PhysicalPosition::new(100, 100),
            PhysicalPosition::new(-100, -100),
            tauri::PhysicalSize::new(100, 80),
            tauri::PhysicalSize::new(200, 120),
        );
        assert_eq!(oversized, PhysicalPosition::new(-100, -100));
    }
}
