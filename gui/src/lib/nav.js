/**
 * The app's navigation model.
 *
 * The old build had two tabs — 麦克风 and 接收器快捷键 — which between them
 * described about a third of what the app actually does. The pie menu, the
 * Claude Code integration, autostart, close-to-tray and the appearance of the
 * app itself had no presence in the window at all; several existed only as
 * tray-menu items or global hotkeys with nothing on screen to discover them
 * from. This file is the fix: every capability has exactly one home, and the
 * two tiers say plainly which ones are about the hardware and which are about
 * the app.
 *
 * The 设备 tier is partly data-driven: sections 2..n-1 come from the connected
 * model's own `Setting.group` values, so a new model that declares a new group
 * gets a section without anyone editing this file.
 */

/** Sections that exist whether or not a microphone is connected. */
export const APP_SECTIONS = [
  { id: "input", label: "敲击与按键", icon: "tap" },
  { id: "pie", label: "快捷菜单", icon: "pie" },
  { id: "prefs", label: "偏好设置", icon: "sliders" },
];

/** Icons for the groups the protocol layer declares today; anything new falls
 *  back to the generic settings glyph rather than rendering nothing. */
const GROUP_ICONS = { 音频: "audio", 电源与启动: "power", 设备: "chip" };

/** Build the 设备 tier for the groups the selected model declares. */
export function deviceSections(groups) {
  return [
    { id: "overview", label: "概览", icon: "gauge" },
    ...groups.map((g) => ({ id: `group:${g}`, label: g, icon: GROUP_ICONS[g] ?? "sliders" })),
    { id: "info", label: "设备信息", icon: "box" },
  ];
}
