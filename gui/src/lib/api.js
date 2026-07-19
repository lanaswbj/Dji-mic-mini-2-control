import { invoke } from "@tauri-apps/api/core";

/** Fetch a full UI snapshot for the given device id (or the sole device). */
export function snapshot(device) {
  return invoke("snapshot", { device: device ?? null });
}

/** Change one setting on a device. Resolves on success, rejects with a message. */
export function setSetting(device, setting, value) {
  return invoke("set_setting", { device, setting, value });
}

/**
 * Change one setting on a specific transmitter slot (0-based) of a device —
 * for a setting that targets one TX individually rather than mirroring
 * across both (currently just Voice Tone).
 */
export function setTxSetting(device, tx, setting, value) {
  return invoke("set_tx_setting", { device, tx, setting, value });
}

/** Fetch the Linux udev-rules helper text. */
export function udevHelp() {
  return invoke("udev_help");
}

/** Install the WinUSB driver for the receiver's control interface (Windows only). */
export function installUsbDriver() {
  return invoke("install_usb_driver");
}

export function receiverShortcutStatus() {
  return invoke("receiver_shortcut_status");
}

export function receiverShortcutStart() {
  return invoke("receiver_shortcut_start");
}

export function receiverShortcutStop() {
  return invoke("receiver_shortcut_stop");
}

/** Test-only: whether the pairing button was pressed in the last ~700ms. */
export function pairingButtonTestActive() {
  return invoke("pairing_button_test_active");
}

/** Test-only: the most recently detected mic-tap group (1/2/3 taps). */
export function micTapTestStatus() {
  return invoke("mic_tap_test_status");
}
