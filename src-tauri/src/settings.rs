use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::RwLock};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, Position, WebviewWindow};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct TaskbarSettings {
    pub display_mode: String,
    pub offset: i32,
    pub window_mode: String,
    pub show_chart: bool,
    pub font_size: u32,
    pub opacity: u32,
    pub always_on_top: bool,
    pub position_locked: bool,
    pub idle_after_seconds: u32,
    pub chart_width: u32,
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
            show_chart: true,
            font_size: 11,
            opacity: 92,
            always_on_top: true,
            position_locked: false,
            idle_after_seconds: 15,
            chart_width: 40,
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
            || !matches!(self.theme.as_str(), "dark" | "light")
            || !(10..=16).contains(&self.font_size)
            || !(30..=100).contains(&self.opacity)
            || !(-5_000..=5_000).contains(&self.offset)
            || !matches!(self.idle_after_seconds, 15 | 30 | 60)
            || !(24..=80).contains(&self.chart_width)
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
        text + if self.show_chart {
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

pub fn apply_window_settings(
    window: &WebviewWindow,
    settings: &TaskbarSettings,
) -> Result<(), String> {
    if settings.window_mode == "taskbar" {
        window.set_always_on_top(false).map_err(|e| e.to_string())?;
        crate::taskbar::embed_taskbar(window, settings.offset, settings.width())
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
        let monitors = window.available_monitors().map_err(|e| e.to_string())?;
        let current_monitor = window.current_monitor().map_err(|e| e.to_string())?;
        let size = window.outer_size().map_err(|e| e.to_string())?;
        let target = settings.floating_x.zip(settings.floating_y);
        let monitor = monitors
            .iter()
            .find(|m| {
                let a = m.work_area();
                target.is_some_and(|(x, y)| {
                    x >= a.position.x
                        && x < a.position.x + a.size.width as i32
                        && y >= a.position.y
                        && y < a.position.y + a.size.height as i32
                })
            })
            .or(current_monitor.as_ref())
            .or_else(|| monitors.first());
        if let Some(monitor) = monitor {
            let a = monitor.work_area();
            let max_x = (a.position.x + a.size.width as i32 - size.width as i32).max(a.position.x);
            let max_y =
                (a.position.y + a.size.height as i32 - size.height as i32).max(a.position.y);
            let (x, y) = target.unwrap_or((max_x - 16, max_y - 16));
            window
                .set_position(Position::Physical(PhysicalPosition::new(
                    x.clamp(a.position.x, max_x),
                    y.clamp(a.position.y, max_y),
                )))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
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
    show_chart: Option<bool>,
    font_size: Option<u32>,
    opacity: Option<u32>,
    always_on_top: Option<bool>,
    position_locked: Option<bool>,
    theme: Option<String>,
    idle_after_seconds: Option<u32>,
    chart_width: Option<u32>,
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
        show_chart,
        font_size,
        opacity,
        always_on_top,
        position_locked,
        theme,
        idle_after_seconds,
        chart_width,
        offset
    );
    next.validate()?;
    let window = app.get_webview_window("main").ok_or("监控窗口不存在")?;
    if let Err(error) =
        apply_window_settings(&window, &next).and_then(|_| persist_taskbar_settings(&next))
    {
        let _ = apply_window_settings(&window, &current);
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
    }
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
        assert_eq!(s.width(), 142.0);
        s.display_mode = "codex".into();
        assert_eq!(s.width(), 104.0);
        s.display_mode = "claude".into();
        assert_eq!(s.width(), 104.0);
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
}
