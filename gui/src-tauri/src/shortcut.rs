//! Receiver button shortcut remap (short-press -> Fn+Control, blocking the
//! system volume change). The previous macOS implementation relied on a
//! bundled native engine (CGEventTap + hidutil) that has no Windows
//! equivalent yet, so this keeps the command surface and UI toggle in place
//! but always reports itself unavailable until a Windows implementation
//! (e.g. a low-level keyboard hook) is written.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatus {
    pub available: bool,
    pub running: bool,
    pub error: String,
}

#[tauri::command]
pub fn receiver_shortcut_status() -> ShortcutStatus {
    ShortcutStatus::default()
}

#[tauri::command]
pub fn receiver_shortcut_start() -> Result<(), String> {
    Err("接收器按键映射尚未在 Windows 上实现".into())
}

#[tauri::command]
pub fn receiver_shortcut_stop() -> Result<(), String> {
    Err("接收器按键映射尚未在 Windows 上实现".into())
}
