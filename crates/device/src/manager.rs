//! Discovery and orchestration of all connected devices.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

use protocol::{model_for_usb, DeviceModel, DeviceStatus, Setting, V2Target};
use serde::Serialize;

use crate::actor::{self, now_millis, Actor};
use crate::error::{DeviceError, Result};
use crate::platform::{current_os, Os};

/// A device is "connected" only if a heartbeat arrived within this window;
/// otherwise it is present on the bus but not streaming (e.g. powered off).
/// Real heartbeat timing is bursty enough that anything shorter flickers.
const LIVE_WINDOW_MS: u64 = 1500;

/// One tracked, opened device.
struct Handle {
    model: &'static dyn DeviceModel,
    actor: Actor,
}

impl Handle {
    /// True if a heartbeat was decoded within [`LIVE_WINDOW_MS`].
    fn is_live(&self) -> bool {
        let last = self.actor.last_seen.load(Ordering::Relaxed);
        last != 0 && now_millis().saturating_sub(last) <= LIVE_WINDOW_MS
    }
}

/// Just enough per-transmitter state for a device-list row's battery
/// indicator — a trimmed-down [`protocol::TxInfo`] rather than the whole
/// thing, since that's all list/picker UI needs without fetching each
/// device's full status.
#[derive(Debug, Clone, Serialize)]
pub struct TxBattery {
    pub battery: Option<u8>,
    pub charging: bool,
}

/// A lightweight description of a device for lists and pickers.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceSummary {
    pub id: String,
    pub model_id: String,
    pub model_name: String,
    pub pictogram_key: String,
    pub connected: bool,
    pub rx_serial: Option<String>,
    /// Battery/charging state for each TX slot, `None` where no transmitter
    /// is connected there.
    pub tx: [Option<TxBattery>; 2],
}

/// The result of scanning the bus, used to decide whether to show setup help.
#[derive(Debug, Clone, Serialize)]
pub struct Probe {
    pub os: Os,
    /// Matching devices seen on the bus.
    pub present: usize,
    /// Matching devices we could open.
    pub accessible: usize,
    /// A device was present but could not be opened (missing udev rule on
    /// Linux, or missing WinUSB driver on Windows).
    pub permission_issue: bool,
}

/// Owns every device task and mediates access from the front-ends.
pub struct DeviceManager {
    handles: Mutex<HashMap<String, Handle>>,
    probe: Mutex<Probe>,
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceManager {
    pub fn new() -> Self {
        Self {
            handles: Mutex::new(HashMap::new()),
            probe: Mutex::new(Probe {
                os: current_os(),
                present: 0,
                accessible: 0,
                permission_issue: false,
            }),
        }
    }

    /// Enumerate the bus, dropping vanished devices and adopting new ones.
    ///
    /// Cheap and idempotent — call it periodically (the GUI) or once before an
    /// action (the CLI).
    pub fn refresh(&self) {
        let debug_file_enabled = Path::new("/tmp/djimic-debug-enable").exists();
        let debug = std::env::var_os("DJIMIC_DEBUG").is_some() || debug_file_enabled;
        let mut debug_file = debug_file_enabled
            .then(|| {
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("/tmp/djimic-protocol.log")
                    .ok()
            })
            .flatten();
        let mut handles = self.handles.lock().unwrap();
        handles.retain(|_, h| h.actor.alive.load(Ordering::Relaxed));

        let mut present = 0;
        let mut permission_issue = false;

        let devices = match nusb::list_devices() {
            Ok(d) => d,
            Err(error) => {
                if debug {
                    eprintln!("[usb] enumeration failed: {error:?}");
                    if let Some(file) = debug_file.as_mut() {
                        let _ = writeln!(file, "[usb] enumeration failed: {error:?}");
                    }
                }
                return;
            }
        };

        for info in devices {
            if debug {
                let line = format!(
                    "[usb] {:04x}:{:04x} product={:?} serial={:?}",
                    info.vendor_id(),
                    info.product_id(),
                    info.product_string(),
                    info.serial_number(),
                );
                eprintln!("{line}");
                if let Some(file) = debug_file.as_mut() {
                    let _ = writeln!(file, "{line}");
                }
            }
            let Some(model) = model_for_usb(info.vendor_id(), info.product_id()) else {
                continue;
            };
            present += 1;
            let id = device_id(
                info.serial_number(),
                info.bus_number(),
                info.device_address(),
            );
            if handles.contains_key(&id) {
                continue;
            }

            let opened = info.open().and_then(|dev| {
                for &iface in model.detach_interfaces() {
                    let _ = dev.detach_kernel_driver(iface);
                }
                dev.detach_and_claim_interface(model.interface())
            });

            match opened {
                Ok(iface) => {
                    if debug {
                        eprintln!("[usb] opened {id} as {}", model.id());
                        if let Some(file) = debug_file.as_mut() {
                            let _ = writeln!(file, "[usb] opened {id} as {}", model.id());
                        }
                    }
                    let actor = actor::spawn(model, iface);
                    handles.insert(id, Handle { model, actor });
                }
                Err(e) => {
                    if debug {
                        eprintln!("[usb] failed to open {id}: {e:?}");
                        if let Some(file) = debug_file.as_mut() {
                            let _ = writeln!(file, "[usb] failed to open {id}: {e:?}");
                        }
                    }
                    // PermissionDenied is Linux's udev-rule case; Unsupported is
                    // what nusb's Windows backend returns when the interface
                    // isn't bound to WinUSB yet (see driver::install).
                    if matches!(e.kind(), ErrorKind::PermissionDenied | ErrorKind::Unsupported) {
                        permission_issue = true;
                    }
                }
            }
        }

        *self.probe.lock().unwrap() = Probe {
            os: current_os(),
            present,
            accessible: handles.len(),
            permission_issue,
        };
    }

    /// The most recent bus-scan result.
    pub fn probe(&self) -> Probe {
        self.probe.lock().unwrap().clone()
    }

    /// Summaries of all currently tracked devices.
    pub fn list(&self) -> Vec<DeviceSummary> {
        let handles = self.handles.lock().unwrap();
        let mut out: Vec<DeviceSummary> = handles
            .iter()
            .map(|(id, h)| {
                let live = h.is_live();
                let st = h.actor.status.lock().unwrap();
                DeviceSummary {
                    id: id.clone(),
                    model_id: h.model.id().to_string(),
                    model_name: h.model.name().to_string(),
                    pictogram_key: h.model.pictogram_key().to_string(),
                    connected: live,
                    rx_serial: st.rx.as_ref().and_then(|r| r.serial.clone()),
                    tx: std::array::from_fn(|i| {
                        st.tx[i].as_ref().map(|t| TxBattery {
                            battery: t.battery,
                            charging: t.charging.unwrap_or(false),
                        })
                    }),
                }
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// The id of the sole tracked device, if exactly one is present.
    pub fn only_device(&self) -> Option<String> {
        let handles = self.handles.lock().unwrap();
        if handles.len() == 1 {
            handles.keys().next().cloned()
        } else {
            None
        }
    }

    /// A clone of the latest status for `id`.
    pub fn status(&self, id: &str) -> Result<DeviceStatus> {
        let handles = self.handles.lock().unwrap();
        let h = handles
            .get(id)
            .ok_or_else(|| DeviceError::NotFound(id.to_string()))?;
        let live = h.is_live();
        let mut status = h.actor.status.lock().unwrap().clone();
        // Reflect live streaming, not merely "has ever connected".
        status.connected = live;
        Ok(status)
    }

    /// The settings descriptors for `id`'s model.
    pub fn settings(&self, id: &str) -> Result<&'static [Setting]> {
        let handles = self.handles.lock().unwrap();
        let h = handles
            .get(id)
            .ok_or_else(|| DeviceError::NotFound(id.to_string()))?;
        Ok(h.model.settings())
    }

    /// Apply `value` to `setting` on device `id`, blocking until acknowledged.
    ///
    /// For a setting addressed to one specific transmitter (its
    /// [`V2Target`] is `Tx`, e.g. Voice Tone), use
    /// [`set_tx`](Self::set_tx) instead.
    pub fn set(&self, id: &str, setting: &str, value: &str) -> Result<()> {
        self.send_set(id, setting, value, None)
    }

    /// Apply `value` to `setting` on transmitter slot `tx` (0-based) of
    /// device `id`, blocking until acknowledged. Only valid for a setting
    /// addressed to one specific transmitter (its [`V2Target`] is `Tx`, e.g.
    /// Voice Tone) — every other setting mirrors across TX or targets the
    /// receiver and should use [`set`](Self::set).
    pub fn set_tx(&self, id: &str, tx: usize, setting: &str, value: &str) -> Result<()> {
        self.send_set(id, setting, value, Some(tx))
    }

    fn send_set(&self, id: &str, setting: &str, value: &str, tx: Option<usize>) -> Result<()> {
        let cmd_tx = {
            let handles = self.handles.lock().unwrap();
            let h = handles
                .get(id)
                .ok_or_else(|| DeviceError::NotFound(id.to_string()))?;

            // Validate up-front for clear errors without a round-trip.
            let desc = h
                .model
                .settings()
                .iter()
                .find(|s| s.id == setting)
                .ok_or_else(|| DeviceError::UnknownSetting(setting.to_string()))?;
            if desc.option(value).is_none() {
                return Err(DeviceError::UnknownValue {
                    setting: setting.to_string(),
                    value: value.to_string(),
                });
            }
            match (desc.v2_target, tx) {
                (V2Target::Tx, None) => return Err(DeviceError::RequiresTx(setting.to_string())),
                (V2Target::Fixed(_), Some(_)) => {
                    return Err(DeviceError::UnexpectedTx(setting.to_string()))
                }
                _ => {}
            }
            if tx.is_some()
                && matches!(desc.v2_target, V2Target::Tx | V2Target::TxOrBroadcast)
                && h.actor.status.lock().unwrap().protocol_version == Some(1)
            {
                return Err(DeviceError::RequiresV2(setting.to_string()));
            }
            h.actor.cmd_tx.clone()
        };

        let (resp_tx, resp_rx) = std::sync::mpsc::channel();
        cmd_tx
            .send_blocking(actor::Command {
                setting_id: setting.to_string(),
                value: value.to_string(),
                tx,
                resp: resp_tx,
            })
            .map_err(|_| DeviceError::Disconnected)?;
        resp_rx.recv().map_err(|_| DeviceError::Disconnected)?
    }
}

/// A stable per-session device id: the USB serial if present, else bus/address.
fn device_id(serial: Option<&str>, bus: u8, address: u8) -> String {
    match serial {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => format!("usb-{bus}-{address}"),
    }
}
