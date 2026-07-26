/**
 * Which magnetic front cover each transmitter is wearing.
 *
 * Purely cosmetic and unknowable over USB — the receiver has no idea what
 * colour shell you clipped on — so it's a local preference the user sets, and
 * it's keyed by serial (so it follows a specific transmitter between sessions)
 * with a slot-number fallback for when the serial hasn't been reported yet.
 */

const KEY = "dji-mic-mini-2-covers";

function load() {
  try {
    return JSON.parse(globalThis.localStorage?.getItem(KEY) ?? "{}");
  } catch {
    return {};
  }
}

class Covers {
  map = $state(load());

  get(tx, index) {
    return this.map[tx?.serial] ?? this.map[`slot-${index}`] ?? (index === 0 ? "obsidian-black" : "glaze-white");
  }

  set(tx, index, color) {
    this.map = {
      ...this.map,
      [`slot-${index}`]: color,
      ...(tx?.serial ? { [tx.serial]: color } : {}),
    };
    try {
      globalThis.localStorage?.setItem(KEY, JSON.stringify(this.map));
    } catch {
      /* storage disabled — the choice just won't survive a restart */
    }
  }
}

export const covers = new Covers();
