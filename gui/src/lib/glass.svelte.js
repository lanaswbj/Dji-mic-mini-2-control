/**
 * Window translucency preference.
 *
 * The window is transparent and carries a Windows 11 **Acrylic** backdrop
 * (`windowEffects` in tauri.conf.json), and app.css builds every layer out of
 * `--glass-*` alphas over it. This module is the switch between:
 *
 *   on  — `--glass-*` at their designed alphas and `--window-bg: transparent`,
 *         so the backdrop shows through
 *   off — `--glass-*` flattened to 1 and `--window-bg` opaque, so the page
 *         covers the backdrop completely (the `[data-translucent="off"]` block
 *         in app.css)
 *
 * ## Why this no longer touches the OS at all
 *
 * It used to call `setEffects`/`clearEffects` to add and remove the backdrop,
 * and turning it back **on** did not visibly work: the window had to be
 * minimised and restored before the glass appeared. That is not a Tauri bug and
 * not a permission problem. `DWMWA_SYSTEMBACKDROP_TYPE` is written correctly,
 * but DWM does not recompose an already-visible window's frame just because the
 * attribute changed — it needs a frame recalculation, which minimise/restore (or
 * a resize, or `SetWindowPos(…, SWP_FRAMECHANGED)`) happens to force. The
 * startup path was never affected because `windowEffects` in tauri.conf.json is
 * applied at window *creation*, before the window is ever shown.
 *
 * So the backdrop is applied once, at creation, and **never removed**. The
 * toggle is CSS only: turning glass off makes the page opaque, which hides the
 * backdrop exactly as completely as removing it would, and turning it on is one
 * attribute flip that lands on the next frame with nothing left to go wrong.
 *
 * **The same quirk bites at startup, and that half is fixed in Rust.** The
 * window is created with `"visible": false` and revealed later (so autostart can
 * come straight up into the tray), so the backdrop attribute lands on a window
 * DWM has never composed — and every launch came up opaque until the user
 * minimised and restored it. `main.rs`'s `reveal_backdrop` re-applies the effect
 * and forces a frame recalculation (`SetWindowPos(…, SWP_FRAMECHANGED)`) on the
 * two paths that reveal the window. That is the *only* place the nudge is
 * needed, because it is the only place the window changes visibility; a
 * preference toggle no longer changes anything the compositor cares about.
 *
 * Keeping the OS calls here instead — with a frame-change nudge after each one —
 * would buy just one thing: DWM stops compositing an acrylic nobody can see, a
 * small idle GPU cost. It is not worth putting a compositor-timing workaround on
 * the path a user touches, and it would need `core:window:allow-set-effects` put
 * back in capabilities/default.json, which was removed along with the calls.
 *
 * ## The rule that survives from the previous version
 *
 * **Both halves must always agree.** A backdrop under opaque CSS is an effect
 * nobody can see; translucent CSS with no backdrop is a window you can see the
 * raw desktop through, unblurred. The second is the one that keeps happening,
 * and it is why `--window-bg` exists: flattening the material alphas alone left
 * the `--panel-gap` gutter transparent, because the gutter paints no material at
 * all — it is `body`.
 *
 * Persisted in localStorage next to the theme, for the same reason: a
 * per-machine display preference, not device state.
 *
 * Acrylic is Windows 11 only and follows the *system* light/dark preference
 * rather than this app's 外观 override, so someone who forces the opposite theme
 * gets a backdrop tinted the other way. That is how every backdrop-backed
 * Windows app behaves, and the material alphas leave enough contrast margin that
 * text survives it; the escape hatch if they don't like it is this toggle.
 */

const KEY = "dji-mic-translucent";

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

  /** Synchronous and infallible by construction — there is no IPC left here.
   *  Safe to call from the pie-menu window too: that one forces its own
   *  `background: transparent !important`, so the attribute changes nothing
   *  there. */
  apply() {
    document.documentElement.setAttribute("data-translucent", this.enabled ? "on" : "off");
  }
}

export const glass = new Glass();
