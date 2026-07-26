/**
 * Window translucency preference.
 *
 * The window itself is transparent and carries a Mica backdrop (declared in
 * tauri.conf.json), and app.css builds every chrome layer out of `--glass-*`
 * alphas over that backdrop. This module is the single switch between the two
 * states, and it has to move *both* halves together:
 *
 *   on  — Mica applied, `--glass-*` at their designed alphas
 *   off — Mica cleared, `--glass-*` flattened to 1 (see the
 *         `[data-translucent="off"]` block in app.css)
 *
 * Clearing only one half would be visibly wrong in either direction: Mica with
 * opaque CSS is an effect nobody can see, and translucent CSS with no Mica is a
 * window you can see the desktop straight through unblurred.
 *
 * Persisted in localStorage next to the theme, for the same reason — it is a
 * per-machine display preference, not device state.
 *
 * Mica is Windows 11 only and follows the *system* light/dark preference rather
 * than this app's 外观 override, so someone who forces the opposite theme gets a
 * backdrop tinted the other way. That is how every Mica-backed Windows app
 * behaves, and the material alphas are conservative enough (chrome 0.66,
 * content 0.86) that text contrast survives it; the escape hatch if they don't
 * like it is this very toggle.
 */

import { getCurrentWindow, Effect } from "@tauri-apps/api/window";

const KEY = "dji-mic-translucent";

/** Only the main window is Mica-backed. The pie-menu overlay shares this
 *  bundle but is a fully transparent always-on-top surface drawn over the
 *  desktop — giving it a window material would paint a rectangle behind an
 *  overlay whose whole point is not having one. */
const isMain = getCurrentWindow().label === "main";

function stored() {
  try {
    return globalThis.localStorage?.getItem(KEY) !== "off";
  } catch {
    return true;
  }
}

class Glass {
  enabled = $state(stored());

  set(next) {
    this.enabled = next;
    try {
      globalThis.localStorage?.setItem(KEY, next ? "on" : "off");
    } catch {
      /* private mode / storage disabled — the choice just won't persist */
    }
    this.apply();
  }

  apply() {
    // The attribute drives the CSS half and is safe everywhere; the effect
    // call is the OS half and is main-window-only.
    document.documentElement.setAttribute(
      "data-translucent",
      this.enabled ? "on" : "off",
    );
    if (!isMain) return;
    const win = getCurrentWindow();
    const p = this.enabled
      ? win.setEffects({ effects: [Effect.Mica] })
      : win.clearEffects();
    // Not fatal and not worth a toast: on Windows 10 (or any platform without
    // Mica) the request simply fails and the window stays opaque, which is
    // precisely the fallback the CSS already renders correctly.
    p?.catch(() => {});
  }
}

export const glass = new Glass();
