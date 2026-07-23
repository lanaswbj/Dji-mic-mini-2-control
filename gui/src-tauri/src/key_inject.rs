//! Simulates keystrokes system-wide via `SendInput`, so a pie-menu slot can
//! act on whatever application currently has focus — the same mechanism a
//! hardware keyboard uses, so it works uniformly across any target app
//! without needing per-app integration.
//!
//! `press_voice_toggle` sends a single, complete Ctrl+Win+Shift press
//! (down then up, like every other function here) rather than a held
//! modifier combo — real usage showed the target app's own voice-input
//! hotkey is itself a toggle (press once to start recording, press again to
//! stop), not a press-and-hold-to-talk gesture. An earlier version instead
//! held a Win+Ctrl combo down across the whole dictation (key-down-only at
//! the start, key-up-only at a later pairing-button press — the two calls
//! could be arbitrarily far apart in time), which is why
//! `pie_menu::VOICE_HOLD_ACTIVE` is still named for a "hold" even though
//! nothing is physically held anymore; it now just tracks "has voice input
//! been toggled on and not yet toggled off," a purely logical state this
//! app has no way to verify against the target app's own actual state (see
//! that flag's doc comment on the timeout that exists because of that gap).

#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_DOWN, VK_LWIN, VK_RETURN, VK_SHIFT, VK_UP,
};

#[cfg(windows)]
fn send(inputs: &[INPUT]) {
    unsafe {
        SendInput(inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(windows)]
fn key_input(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(windows)]
fn press_key(vk: VIRTUAL_KEY) {
    send(&[
        key_input(vk, KEYBD_EVENT_FLAGS(0)),
        key_input(vk, KEYEVENTF_KEYUP),
    ]);
}

#[cfg(windows)]
pub fn press_down_arrow() {
    press_key(VK_DOWN);
}

#[cfg(windows)]
pub fn press_up_arrow() {
    press_key(VK_UP);
}

#[cfg(windows)]
pub fn press_enter() {
    press_key(VK_RETURN);
}

/// Types `text` via Unicode key events (`KEYEVENTF_UNICODE`) rather than
/// virtual-key codes, so it isn't limited to characters that have a virtual
/// key on the current keyboard layout.
#[cfg(windows)]
pub fn type_text(text: &str) {
    let mut inputs = Vec::with_capacity(text.len() * 2);
    for unit in text.encode_utf16() {
        let unicode_input = |flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: unit,
                    dwFlags: KEYEVENTF_UNICODE | flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        inputs.push(unicode_input(KEYBD_EVENT_FLAGS(0)));
        inputs.push(unicode_input(KEYEVENTF_KEYUP));
    }
    send(&inputs);
}

// An earlier held-combo version added KEYEVENTF_EXTENDEDKEY here at one
// point (an attempt to fix a suspected stuck-Win-key/window-snap flicker
// after voice input) and it broke recognition of the hold entirely — kept
// as a warning against reintroducing that flag if this toggle ever stops
// being recognized either: whatever's listening for it is sensitive to the
// synthesized event's exact shape.
#[cfg(windows)]
pub fn press_voice_toggle() {
    send(&[
        key_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        key_input(VK_LWIN, KEYBD_EVENT_FLAGS(0)),
        key_input(VK_SHIFT, KEYBD_EVENT_FLAGS(0)),
        key_input(VK_SHIFT, KEYEVENTF_KEYUP),
        key_input(VK_LWIN, KEYEVENTF_KEYUP),
        key_input(VK_CONTROL, KEYEVENTF_KEYUP),
    ]);
}

#[cfg(not(windows))]
pub fn press_down_arrow() {}
#[cfg(not(windows))]
pub fn press_up_arrow() {}
#[cfg(not(windows))]
pub fn press_enter() {}
#[cfg(not(windows))]
pub fn type_text(_text: &str) {}
#[cfg(not(windows))]
pub fn press_voice_toggle() {}
