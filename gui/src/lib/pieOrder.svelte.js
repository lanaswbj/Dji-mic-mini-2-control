/**
 * The pie menu's slot order, as chosen by the user.
 *
 * The order is the ergonomics here, which is why this is the one list in the
 * app worth making rearrangeable: the overlay is driven by arrow keys, by mic
 * taps and by the receiver's pairing button, so a slot's position decides how
 * many steps of travel it costs. Whichever action you actually use should be
 * the one next to where the selection starts.
 *
 * ## What an entry is
 *
 * A **stable slot index** — the `index` field of `pie_menu.rs`'s `SLOTS`, which
 * is also what `pie_menu_select`'s `match` is written against. Reordering
 * therefore never changes what a slot *does*; it changes only where it is
 * drawn. `PieMenu.svelte` maps the picked screen position back through this
 * order before it calls `pieMenuSelect`, so the backend keeps receiving the
 * same number it always did and needs no notion of order at all.
 *
 * ## Two constraints, both enforced on read
 *
 * 1. **The close slot stays last.** `PieMenu.svelte` recognises "close" as
 *    `index === ITEMS.length - 1` — it is the one slot that dismisses the
 *    overlay instead of firing into the window behind it, and a question
 *    overlay reuses the same last-slot convention. Letting it be dragged
 *    elsewhere would make some other action close the menu.
 * 2. **It must be a permutation.** A stored order that has lost or duplicated a
 *    slot — an older build with a different slot count, a hand-edited value —
 *    would silently hide an action. Anything that isn't a clean permutation of
 *    every slot is discarded whole rather than repaired, because a
 *    half-understood order is worse than the default one.
 *
 * ## Why localStorage rather than the backend
 *
 * The overlay and the main window are two WebViews of the same origin, so they
 * share one localStorage. The main window writes; the overlay re-reads on every
 * open (`refresh`), which is also the only moment the order can matter. That
 * keeps the whole feature on one side of the IPC boundary — no new command, no
 * config file, and nothing that can disagree with itself.
 */

import { DEFAULT_ITEMS } from "./pieIcons.js";

const KEY = "dji-mic-pie-order";

/** Slot count and the index pinned to the end, both derived rather than
 *  restated — adding a seventh slot must not need an edit here. */
const COUNT = DEFAULT_ITEMS.length;
const PINNED_LAST = COUNT - 1;

const natural = () => Array.from({ length: COUNT }, (_, i) => i);

/** A permutation of every slot, ending on the close slot, or null. */
function validate(value) {
  if (!Array.isArray(value) || value.length !== COUNT) return null;
  if (value[COUNT - 1] !== PINNED_LAST) return null;
  const seen = new Set();
  for (const v of value) {
    if (!Number.isInteger(v) || v < 0 || v >= COUNT || seen.has(v)) return null;
    seen.add(v);
  }
  return value;
}

function stored() {
  try {
    const raw = globalThis.localStorage?.getItem(KEY);
    return (raw ? validate(JSON.parse(raw)) : null) ?? natural();
  } catch {
    return natural();
  }
}

class PieOrder {
  /** Stable slot indices, in the order they should be drawn. */
  indices = $state(stored());

  /** Re-read from storage. The overlay window has its own JS realm, so it
   *  cannot see the main window's in-memory state — only the storage the two
   *  share. Called on every `pie-menu:open`. */
  refresh() {
    this.indices = stored();
  }

  /** Reorder any array that is parallel to `SLOTS` (items, icons, labels). */
  arrange(list) {
    return this.indices.map((i) => list[i]);
  }

  /** Commit a new order. Rejected silently if it isn't a valid permutation —
   *  the caller is a drag gesture, and there is no useful way to explain to a
   *  drag that it produced a list of the wrong length. */
  set(indices) {
    const valid = validate(indices);
    if (!valid) return false;
    this.indices = [...valid];
    try {
      globalThis.localStorage?.setItem(KEY, JSON.stringify(this.indices));
    } catch {
      /* private mode / storage disabled — the order just won't persist */
    }
    return true;
  }

  reset() {
    this.indices = natural();
    try {
      globalThis.localStorage?.removeItem(KEY);
    } catch {
      /* same */
    }
  }

  /** Whether the user has moved anything, so a "restore" affordance can appear
   *  only when there is something to restore. */
  get customised() {
    return this.indices.some((v, i) => v !== i);
  }

  /** How many leading slots may be dragged — everything but the pinned close
   *  slot at the end. */
  get movable() {
    return COUNT - 1;
  }
}

export const pieOrder = new PieOrder();
