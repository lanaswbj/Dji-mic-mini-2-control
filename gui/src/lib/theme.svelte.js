/**
 * Appearance preference.
 *
 * "system" (the default) leaves `data-theme` off the root entirely so the
 * `prefers-color-scheme` block in app.css decides; the two explicit choices
 * stamp the attribute, which is written to beat the media query in both
 * directions. Persisted in localStorage — it's a per-machine display
 * preference, not device state, so it has no business on the USB bus.
 */

const KEY = "dji-mic-theme";
/** Shaped as `Segmented`'s options so the appearance picker can pass it
 *  straight through without restating the same three labels. */
export const THEMES = [
  { value: "system", label: "跟随系统" },
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
];

function stored() {
  try {
    const v = globalThis.localStorage?.getItem(KEY);
    return THEMES.some((t) => t.value === v) ? v : "system";
  } catch {
    return "system";
  }
}

class Theme {
  value = $state(stored());

  set(next) {
    this.value = next;
    try {
      globalThis.localStorage?.setItem(KEY, next);
    } catch {
      /* private mode / storage disabled — the choice just won't persist */
    }
    this.apply();
  }

  apply() {
    const root = document.documentElement;
    if (this.value === "system") root.removeAttribute("data-theme");
    else root.setAttribute("data-theme", this.value);
  }
}

export const theme = new Theme();
