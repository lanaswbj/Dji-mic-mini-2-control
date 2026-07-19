//! Detects a single short press of the receiver's pairing button.
//!
//! The receiver doesn't report button presses over the vendor control
//! interface (MI_06, the one `device::DeviceManager` talks to) — captured
//! raw frames there never changed on any button press. It turns out the
//! pairing button instead shows up as a standard HID Consumer Control input
//! report on a *separate* USB interface (MI_00), which Windows already
//! exposes as its own HID collection: report id 6, a little-endian u16
//! usage value that goes 1 (pressed) then back to 0 (released). Confirmed
//! by capturing raw HID reports while pressing the physical button.
//!
//! That collection is also exactly what Windows' built-in "HID-compliant
//! consumer control device" driver uses to automatically bump the system
//! volume on every press — the same problem the DJI macOS app solved with
//! `hidutil` (a system-wide usage remap, not scoped to one device). Two
//! approaches were tried to suppress it and both failed: a low-level
//! keyboard hook doesn't catch it (Windows' default handling for HID
//! consumer-page usages happens below the synthesized-keystroke layer, so
//! it never reaches `WH_KEYBOARD_LL`), and the Win32 Raw Input API's
//! `RIDEV_NOLEGACY` flag — which in principle tells Windows not to run its
//! default action for a given (usage page, usage) — turns out to only be
//! valid for the Generic Desktop page (mouse/keyboard, page 0x01); using it
//! here on the Consumer Control page (0x0C) makes
//! `RegisterRawInputDevices` fail outright with `E_INVALIDARG`, silently,
//! since the original code didn't check its return value — meaning
//! *neither* button detection *nor* volume suppression were ever actually
//! working. This module now just reads the button via plain
//! `RIDEV_INPUTSINK` (detection restored); the system volume changing on
//! every press is a known, currently-unsolved side effect.
//!
//! The receiver's power button was also tested this way and never produced
//! any HID report on any of its four collections (Consumer Control,
//! Telephony, and two vendor-defined ones) even on a longer press — it
//! appears to be a direct hardware power toggle with no host-visible short
//! press event, so it isn't handled here.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long the frontend should consider a press "active" after it's
/// detected, so a quick poll-based UI can still catch a momentary flash.
const ACTIVE_WINDOW: Duration = Duration::from_millis(700);

/// The DJI receiver's vendor/product id, used to ignore raw input from any
/// other device that happens to expose the same HID Consumer Control usage
/// (most multimedia keyboards do).
const VID: u32 = 0x2ca3;
const PID: u32 = 0x4011;

pub struct PairingButtonWatcher {
    last_press_millis: AtomicU64,
}

impl PairingButtonWatcher {
    pub fn is_active(&self) -> bool {
        let last = self.last_press_millis.load(Ordering::Relaxed);
        if last == 0 {
            return false;
        }
        now_millis().saturating_sub(last) < ACTIVE_WINDOW.as_millis() as u64
    }

    /// Called from the raw-input thread on every detected press: records the
    /// press for the test indicator, and also mirrors it into the
    /// module-level `LAST_PRESS_MILLIS` so `mic_tap.rs` can suppress the
    /// button's own mechanical click from registering as a shell tap —
    /// see `recently_pressed`.
    fn on_press(&self) {
        let now = now_millis();
        self.last_press_millis.store(now, Ordering::Relaxed);
        LAST_PRESS_MILLIS.store(now, Ordering::Relaxed);
    }

    /// Called on the HID release transition. The physical button has an
    /// audible click on *release* too, not just on press — if the button is
    /// held for longer than the mic-tap suppression window before letting
    /// go, that release click landed outside the window and got picked up
    /// as its own "tap". Refreshing the suppression timestamp here (without
    /// touching the press-only `last_press_millis`/active-indicator state)
    /// covers it.
    fn on_release(&self) {
        LAST_PRESS_MILLIS.store(now_millis(), Ordering::Relaxed);
    }
}

/// Mirrors the most recent press across all `PairingButtonWatcher`
/// instances (there's only ever one in practice) — a plain module-level
/// static so `mic_tap.rs` can check it without needing a reference to the
/// watcher itself.
static LAST_PRESS_MILLIS: AtomicU64 = AtomicU64::new(0);

/// True if the pairing button was pressed within the last `window` — the
/// button's own physical click is picked up by the mic and can otherwise
/// read as a shell tap.
pub fn recently_pressed(window: Duration) -> bool {
    let last = LAST_PRESS_MILLIS.load(Ordering::Relaxed);
    last != 0 && now_millis().saturating_sub(last) < window.as_millis() as u64
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(windows)]
pub fn spawn() -> Arc<PairingButtonWatcher> {
    let watcher = Arc::new(PairingButtonWatcher {
        last_press_millis: AtomicU64::new(0),
    });
    let watcher_thread = watcher.clone();
    std::thread::spawn(move || win32::run(watcher_thread));
    watcher
}

/// Stops intercepting the Consumer Control collection, restoring Windows'
/// default handling (system volume changes again). Called on app quit.
#[cfg(windows)]
pub fn uninstall() {
    win32::stop();
}

#[cfg(not(windows))]
pub fn spawn() -> Arc<PairingButtonWatcher> {
    Arc::new(PairingButtonWatcher {
        last_press_millis: AtomicU64::new(0),
    })
}

#[cfg(not(windows))]
pub fn uninstall() {}

#[tauri::command]
pub fn pairing_button_test_active(watcher: tauri::State<'_, Arc<PairingButtonWatcher>>) -> bool {
    watcher.is_active()
}

#[cfg(windows)]
mod win32 {
    use super::{PairingButtonWatcher, PID, VID};
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::Arc;
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::{
        GetRawInputData, GetRawInputDeviceInfoW, RegisterRawInputDevices, HRAWINPUT, RAWINPUT,
        RAWINPUTDEVICE, RAWINPUTHEADER, RID_DEVICE_INFO, RIDEV_INPUTSINK, RIDEV_REMOVE,
        RIDI_DEVICEINFO, RID_INPUT, RIM_TYPEHID,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
        GetWindowLongPtrW, PostMessageW, PostQuitMessage, RegisterClassExW, SetWindowLongPtrW,
        TranslateMessage, CW_USEDEFAULT, GWLP_USERDATA, HWND_MESSAGE, MSG, WM_CLOSE, WM_DESTROY,
        WM_INPUT, WNDCLASSEXW, WNDCLASS_STYLES,
    };

    const USAGE_PAGE_CONSUMER: u16 = 0x0c;
    const USAGE_CONSUMER_CONTROL: u16 = 0x01;

    /// The hidden message-only window's handle, so `stop()` can ask its
    /// message loop (running on a different thread) to shut down.
    static WINDOW: AtomicIsize = AtomicIsize::new(0);

    pub fn run(watcher: Arc<PairingButtonWatcher>) {
        unsafe {
            let Ok(hinstance) = GetModuleHandleW(None) else {
                return;
            };
            let class_name = w!("DjiMicPairingButtonWnd");

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: WNDCLASS_STYLES::default(),
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            // Ignore failure: benign if this ever runs twice and the class
            // is already registered.
            RegisterClassExW(&wc);

            let Ok(hwnd) = CreateWindowExW(
                Default::default(),
                class_name,
                w!(""),
                Default::default(),
                0,
                0,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                Some(HWND_MESSAGE),
                None,
                Some(hinstance.into()),
                None,
            ) else {
                return;
            };

            let watcher_ptr = Arc::into_raw(watcher);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, watcher_ptr as isize);
            WINDOW.store(hwnd.0 as isize, Ordering::SeqCst);

            register(hwnd, RIDEV_INPUTSINK);

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            WINDOW.store(0, Ordering::SeqCst);
            drop(Arc::from_raw(watcher_ptr));
        }
    }

    /// Ask the message loop to exit, which restores Windows' default
    /// handling for the Consumer Control collection.
    pub fn stop() {
        let raw = WINDOW.load(Ordering::SeqCst);
        if raw != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(raw as _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }

    unsafe fn register(hwnd: HWND, flags: windows::Win32::UI::Input::RAWINPUTDEVICE_FLAGS) {
        let device = RAWINPUTDEVICE {
            usUsagePage: USAGE_PAGE_CONSUMER,
            usUsage: USAGE_CONSUMER_CONTROL,
            dwFlags: flags,
            hwndTarget: if flags == RIDEV_REMOVE { HWND(std::ptr::null_mut()) } else { hwnd },
        };
        let _ = RegisterRawInputDevices(&[device], std::mem::size_of::<RAWINPUTDEVICE>() as u32);
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_INPUT => {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const PairingButtonWatcher;
                if !ptr.is_null() {
                    handle_raw_input(&*ptr, lparam);
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                register(hwnd, RIDEV_REMOVE);
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    unsafe fn handle_raw_input(watcher: &PairingButtonWatcher, lparam: LPARAM) {
        let handle = HRAWINPUT(lparam.0 as *mut _);

        let mut size: u32 = 0;
        GetRawInputData(
            handle,
            RID_INPUT,
            None,
            &mut size,
            std::mem::size_of::<RAWINPUTHEADER>() as u32,
        );
        if size == 0 {
            return;
        }

        let mut buf = vec![0u8; size as usize];
        let copied = GetRawInputData(
            handle,
            RID_INPUT,
            Some(buf.as_mut_ptr() as *mut _),
            &mut size,
            std::mem::size_of::<RAWINPUTHEADER>() as u32,
        );
        if copied == u32::MAX || (copied as usize) < std::mem::size_of::<RAWINPUTHEADER>() {
            return;
        }

        let raw = &*(buf.as_ptr() as *const RAWINPUT);
        if raw.header.dwType != RIM_TYPEHID.0 {
            return;
        }
        if !from_dji_device(raw.header.hDevice.0 as isize) {
            return;
        }

        let hid = &raw.data.hid;
        let report_size = hid.dwSizeHid as usize;
        if report_size < 2 {
            return;
        }
        let report = std::slice::from_raw_parts(hid.bRawData.as_ptr(), report_size);
        if report[1] != 0 {
            watcher.on_press();
        } else {
            watcher.on_release();
        }
    }

    unsafe fn from_dji_device(hdevice: isize) -> bool {
        let mut info = RID_DEVICE_INFO {
            cbSize: std::mem::size_of::<RID_DEVICE_INFO>() as u32,
            ..Default::default()
        };
        let mut size = info.cbSize;
        let ok = GetRawInputDeviceInfoW(
            Some(windows::Win32::Foundation::HANDLE(hdevice as *mut _)),
            RIDI_DEVICEINFO,
            Some(&mut info as *mut _ as *mut _),
            &mut size,
        );
        if ok == u32::MAX {
            return false;
        }
        let hid = info.Anonymous.hid;
        hid.dwVendorId == VID && hid.dwProductId == PID
    }
}
