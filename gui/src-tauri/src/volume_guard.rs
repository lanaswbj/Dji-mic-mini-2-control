//! Neutralizes the pairing button's system-volume side effect.
//!
//! The button's HID Consumer Control report also feeds Windows' own
//! built-in volume handling, and that can't be suppressed at the input
//! level — `RIDEV_NOLEGACY` is documented as valid only for the Generic
//! Desktop (mouse/keyboard) page, and Windows' HID class driver already
//! holds the Consumer Control collection open exclusively for its own
//! handling, so a second exclusive claim from this app isn't possible
//! either (see `pairing_button.rs`'s module doc comment for the full
//! history — two suppression approaches were tried before this file existed
//! and both failed for this same underlying reason).
//!
//! Rather than trying to stop the change from happening, this continuously
//! snapshots the default output device's volume/mute state while idle and
//! forces it back shortly after every pairing-button press — so the actual
//! volume level never drifts. A second, independent background loop keeps
//! the on-screen overlay itself hidden for as long as this app is running at
//! all, rather than reacting to individual presses (a press-triggered,
//! one-shot version was tried first, but the OSD still flashed occasionally
//! for reasons never pinned down, e.g. around the voice-input slot's
//! Win+Ctrl hold).
//!
//! Finding the OSD's window uses the same technique the actual open-source
//! `HideVolumeOSD` tool uses (its source was cloned and read directly to
//! confirm this): Windows 10 renders it as a classic `NativeHWNDHost`
//! top-level window with a `DirectUIHWND` child, while Windows 11
//! (build >= 22000) replaced it with a XAML-hosted flyout instead —
//! `XamlExplorerHostIslandWindow` with a
//! `Windows.UI.Composition.DesktopWindowContentBridge` child named
//! `DesktopWindowXamlSource`.
//!
//! Unlike that tool, this hides the *inner content* window
//! (`SW_HIDE` on `DirectUIHWND`/the XAML bridge child) rather than
//! minimizing the outer top-level host. Two earlier versions minimized the
//! host instead (matching `HideVolumeOSD` itself) and both broke other
//! things: Windows 11 reuses that same `XamlExplorerHostIslandWindow` host
//! across other system flyouts too, not just volume, so a version that
//! never restored it broke *all* of them from reappearing, and a version
//! that minimized-then-restored the host shortly after just made the OSD
//! flash repeatedly instead (restoring a minimized window necessarily shows
//! it again — there's no "restore the internal state without the window
//! becoming visible"). The content child is a much smaller, more local
//! target: hiding it doesn't touch the shared host's own window-manager
//! state at all, so nothing needs restoring afterward, and it doesn't
//! interfere with whatever else reuses the host.

use std::time::Duration;

#[cfg(windows)]
use windows::core::{w, PCWSTR};
#[cfg(windows)]
use windows::Win32::Foundation::HWND;
#[cfg(windows)]
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
#[cfg(windows)]
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
#[cfg(windows)]
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, ShowWindow, SW_HIDE, SW_SHOW};

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

static LAST_LEVEL_BITS: AtomicU32 = AtomicU32::new(0);
static LAST_MUTED: AtomicBool = AtomicBool::new(false);
/// Set while a restore is in flight (or about to be), so the polling loop
/// doesn't overwrite the "known good" snapshot with the post-press,
/// not-yet-restored value.
static RESTORING: AtomicBool = AtomicBool::new(false);
/// Cleared on `uninstall` so the suppression loop stops touching windows
/// once the app is shutting down.
static SUPPRESSING: AtomicBool = AtomicBool::new(true);

#[cfg(windows)]
fn endpoint_volume() -> Option<IAudioEndpointVolume> {
    unsafe {
        // Ignore the result: fine to call repeatedly / already-initialized
        // on this thread, and there's nothing useful to do if it fails
        // beyond what the subsequent calls already handle via `.ok()?`.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
    }
}

/// Looks for a top-level window of `outer_class` with a child of
/// `inner_class` (optionally also matching `inner_name`) and, if found,
/// returns the *child's* handle (the thing actually worth hiding/showing —
/// see the module doc comment for why not the outer host).
#[cfg(windows)]
unsafe fn find_content_child(
    outer_class: PCWSTR,
    inner_class: PCWSTR,
    inner_name: PCWSTR,
) -> Option<HWND> {
    let host = FindWindowExW(None, None, outer_class, PCWSTR::null()).ok()?;
    if host.is_invalid() {
        return None;
    }
    let child = FindWindowExW(Some(host), None, inner_class, inner_name).ok()?;
    if child.is_invalid() {
        return None;
    }
    Some(child)
}

#[cfg(windows)]
unsafe fn find_osd_content() -> Option<HWND> {
    // Windows 11 (build >= 22000): XAML-hosted flyout.
    find_content_child(
        w!("XamlExplorerHostIslandWindow"),
        w!("Windows.UI.Composition.DesktopWindowContentBridge"),
        w!("DesktopWindowXamlSource"),
    )
    // Windows 10 (and possibly older Windows 11 builds): classic Win32 dialog.
    .or_else(|| find_content_child(w!("NativeHWNDHost"), w!("DirectUIHWND"), PCWSTR::null()))
}

#[cfg(windows)]
pub fn spawn() {
    SUPPRESSING.store(true, Ordering::Relaxed);

    // Volume level/mute snapshot-and-restore loop — see restore_after_press.
    std::thread::spawn(|| {
        let Some(endpoint) = endpoint_volume() else {
            return;
        };
        loop {
            if !RESTORING.load(Ordering::Relaxed) {
                unsafe {
                    if let Ok(level) = endpoint.GetMasterVolumeLevelScalar() {
                        LAST_LEVEL_BITS.store(level.to_bits(), Ordering::Relaxed);
                    }
                    if let Ok(muted) = endpoint.GetMute() {
                        LAST_MUTED.store(muted.as_bool(), Ordering::Relaxed);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    });

    // OSD-window suppression loop — runs for as long as the app does (see
    // uninstall), independent of any specific press.
    std::thread::spawn(|| loop {
        if !SUPPRESSING.load(Ordering::Relaxed) {
            return;
        }
        if let Some(child) = unsafe { find_osd_content() } {
            unsafe {
                let _ = ShowWindow(child, SW_HIDE);
            }
        }
        std::thread::sleep(Duration::from_millis(40));
    });
}

/// Called from `pairing_button.rs` on every press: forces the system volume
/// back to the last known-good snapshot shortly after, undoing whatever
/// Windows' own Consumer Control handling just did to it. The delay needs
/// to be long enough that it runs *after* that handling has applied its
/// change (otherwise we'd restore first and it would just reapply after),
/// but short enough that the drift is imperceptible.
#[cfg(windows)]
pub fn restore_after_press() {
    RESTORING.store(true, Ordering::Relaxed);
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(150));
        if let Some(endpoint) = endpoint_volume() {
            let level = f32::from_bits(LAST_LEVEL_BITS.load(Ordering::Relaxed));
            let muted = LAST_MUTED.load(Ordering::Relaxed);
            unsafe {
                let _ = endpoint.SetMasterVolumeLevelScalar(level, std::ptr::null());
                let _ = endpoint.SetMute(muted, std::ptr::null());
            }
        }
        RESTORING.store(false, Ordering::Relaxed);
    });
}

/// Stops the suppression loop and makes a best-effort attempt to leave the
/// OSD content window in a normal, visible state again — called on app
/// quit. A safety net as much as anything: since this only ever hides the
/// small inner content window (not the shared host — see the module doc
/// comment), there shouldn't be anything left to undo, but earlier versions
/// of this file *did* leave the shared host stuck, so this errs on the side
/// of explicitly showing it back.
#[cfg(windows)]
pub fn uninstall() {
    SUPPRESSING.store(false, Ordering::Relaxed);
    unsafe {
        if let Some(child) = find_osd_content() {
            let _ = ShowWindow(child, SW_SHOW);
        }
    }
}

#[cfg(not(windows))]
pub fn spawn() {}

#[cfg(not(windows))]
pub fn restore_after_press() {}

#[cfg(not(windows))]
pub fn uninstall() {}
