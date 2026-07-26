/**
 * What each setting actually does, in one sentence.
 *
 * The protocol layer only knows a setting's `label` (and occasionally a
 * `note` about a side effect) — it has no business carrying user-facing
 * prose, so the explanations live here. A settings screen that lists thirteen
 * switches and explains none of them makes the user guess; guessing on
 * hardware settings is how people end up with a recording they can't use.
 *
 * A missing entry is fine and renders as a bare label — an unexplained
 * setting is better than a wrong explanation, so a new model's new setting
 * stays silent until someone writes its line.
 */
export const SETTING_HELP = {
  "noise-cancel": "强降噪抑制更多环境声，但对人声的处理也更重。",
  "noise-cancel-power": "开启后由发射器实时降低环境噪声，两个发射器可分别设置。",
  "noise-cancel-button": "允许直接按发射器上的按键切换降噪，无需打开本应用。",
  "low-cut": "衰减低频，减少风噪、桌面震动和空调等持续低频声。",
  stereo: "两个发射器分别录入左右声道；开启后无法同时使用安全音轨。",
  "safety-track": "额外录一条低 6dB 的备份音轨，主音轨爆音时可用它救回来。",
  "clip-limiter": "在信号接近削波时自动压低电平，防止突然的大声导致失真。",
  "voice-tone": "调整发射器的音色曲线，按各自的嗓音选择。",
  "auto-off-15m": "接收器闲置 15 分钟后自动关机以省电。",
  "tx-auto-off-15m": "发射器闲置 15 分钟后自动关机以省电。",
  "camera-power": "接入相机后随相机一同开关机。",
  "plug-free": "免驱模式便于直接接入电脑；关闭后可获得更完整的设备功能。",
  "mic-leds": "关闭发射器上的指示灯，适合安静或需要隐蔽的拍摄场合。",
};
