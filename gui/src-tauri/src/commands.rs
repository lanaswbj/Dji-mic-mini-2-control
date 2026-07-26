//! Tauri commands bridging the Svelte front-end to the shared device layer.

use std::sync::Arc;

use device::{DeviceManager, DeviceStatus, DeviceSummary, Os, Probe, Setting};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

/// Everything the UI needs for one render, fetched in a single call.
#[derive(Serialize)]
pub struct Snapshot {
    /// Host operating system.
    pub os: Os,
    /// Latest bus scan (drives the setup-help banner).
    pub probe: Probe,
    /// All tracked devices for the sidebar.
    pub devices: Vec<DeviceSummary>,
    /// Full status of the selected device, if any.
    pub status: Option<DeviceStatus>,
    /// Setting descriptors for the selected device's model.
    pub settings: Vec<Setting>,
}

/// The udev-rules helper contents shown on Linux when access is blocked.
#[derive(Serialize)]
pub struct UdevHelp {
    pub file: String,
    pub rule: String,
    pub steps: Vec<String>,
}

/// Read the current state for `device` (or the sole device when omitted).
#[tauri::command]
pub fn snapshot(device: Option<String>, mgr: State<'_, Arc<DeviceManager>>) -> Snapshot {
    let devices = mgr.list();
    let probe = mgr.probe();
    let selected = device.or_else(|| mgr.only_device());

    let (status, settings) = match &selected {
        Some(id) => (
            mgr.status(id).ok(),
            mgr.settings(id).map(|s| s.to_vec()).unwrap_or_default(),
        ),
        None => (None, Vec::new()),
    };

    Snapshot {
        os: device::current_os(),
        probe,
        devices,
        status,
        settings,
    }
}

/// Change one setting on a device.
#[tauri::command]
pub fn set_setting(
    device: String,
    setting: String,
    value: String,
    mgr: State<'_, Arc<DeviceManager>>,
) -> Result<(), String> {
    mgr.set(&device, &setting, &value)
        .map_err(|e| e.to_string())
}

/// Change one setting on a specific transmitter slot (0-based) of a device —
/// for a setting that targets one TX individually rather than mirroring
/// across both (currently just Voice Tone).
#[tauri::command]
pub fn set_tx_setting(
    device: String,
    tx: usize,
    setting: String,
    value: String,
    mgr: State<'_, Arc<DeviceManager>>,
) -> Result<(), String> {
    mgr.set_tx(&device, tx, &setting, &value)
        .map_err(|e| e.to_string())
}

/// App-level facts the 偏好设置 section needs.
///
/// Everything here is read from the one place that actually owns it — the
/// autostart plugin, `pie_menu`'s registered hotkey, the two loopback
/// listeners' own port constants — rather than restated in the frontend,
/// where it would silently rot the first time one of them changed. The two
/// ports in particular were previously bound with no disclosure anywhere in
/// the UI; a settings screen that names them is the point.
#[derive(Serialize)]
pub struct AppInfo {
    pub version: String,
    pub os: Os,
    pub autostart: bool,
    pub pie_menu_hotkey: String,
    pub hook_port: u16,
    pub permission_port: u16,
}

/// Read app-level state for the preferences screen.
#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        os: device::current_os(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        pie_menu_hotkey: crate::pie_menu::HOTKEY_LABEL.to_string(),
        hook_port: crate::hook_bridge::PORT,
        permission_port: crate::permission_server::PORT,
    }
}

/// Turn launch-at-login on or off. The tray menu has the same toggle; both
/// funnel through the plugin, so whichever one you use the other reflects it
/// on its next rebuild.
#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let auto = app.autolaunch();
    if enabled {
        auto.enable()
    } else {
        auto.disable()
    }
    .map_err(|e| e.to_string())
}

/// The udev rule and instructions for Linux setup.
#[tauri::command]
pub fn udev_help() -> UdevHelp {
    UdevHelp {
        file: device::UDEV_RULE_FILE.to_string(),
        rule: device::udev_rule(),
        steps: device::udev_instructions(),
    }
}

