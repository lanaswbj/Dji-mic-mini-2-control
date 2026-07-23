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

/**
 * Report that the most recently detected tap group was wrong (nothing was
 * actually tapped) — relabels the acoustic event and queues an incremental
 * retrain. Rejects with a Chinese error message if there's no recent group
 * to target (see `TapStatus.active`/`.count`).
 */
export function micTapReportFalsePositive() {
  return invoke("mic_tap_report_false_positive");
}

/**
 * Report that a real tap on the mic shell went undetected — scans the last
 * few seconds for the loudest sound and, if it clears a basic sanity floor,
 * labels it as a tap and queues an incremental retrain.
 */
export function micTapReportFalseNegative() {
  return invoke("mic_tap_report_false_negative");
}

/** Poll target for the incremental-training panel. */
export function micTapTrainingStatus() {
  return invoke("mic_tap_training_status");
}

/** Restore the model that was live before the last accepted incremental update. */
export function micTapRollbackModel() {
  return invoke("mic_tap_rollback_model");
}

/** Discard all on-device adaptation and go back to the model this build shipped with. */
export function micTapRestoreFactoryModel() {
  return invoke("mic_tap_restore_factory_model");
}

/** Hide the pie menu overlay without picking a slot. */
export function pieMenuClose() {
  return invoke("pie_menu_close");
}

/** Confirm a pie menu slot (0-based). Placeholder slots for now. */
export function pieMenuSelect(index) {
  return invoke("pie_menu_select", { index });
}

/**
 * Answer a pending Claude Code question relayed from gui/src-tauri/src/hook_bridge.rs
 * — either a PermissionRequest choice or a single-select AskUserQuestion
 * tool call — with the chosen slot (0-based, same left-to-right order the
 * icons/labels were shown in).
 */
export function pieMenuAnswerQuestion(index) {
  return invoke("pie_menu_answer_question", { index });
}
