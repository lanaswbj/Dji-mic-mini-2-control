/**
 * The device store — one owner for every piece of device state the UI reads.
 *
 * The old build kept all of this inside App.svelte, which meant the shell and
 * the content were the same file and nothing else could read a device value
 * without being handed it through five levels of props. With seven sections
 * that stops scaling, so the polling loop, the optimistic-write bookkeeping,
 * and the merge logic live here and every section imports what it needs.
 *
 * Three behaviors here are deliberate, and each fixes a real defect in the
 * old build:
 *
 *  1. **Polling pauses when the window isn't visible.** Closing to tray used
 *     to leave a 250ms USB poll running forever.
 *  2. **Every optimistic value has a deadline.** A write the device never
 *     confirms used to leave the UI permanently showing a value the hardware
 *     doesn't have. Now it reverts after `OPTIMISTIC_TIMEOUT` and says so.
 *  3. **Write state is observable, and a failure says why.** `writeState(id)`
 *     returns idle/writing/ok/error so a row can show a write in flight, and
 *     every failure also raises a toast carrying the actual reason plus a
 *     retry — a row that only says 未生效 is barely better than the old build
 *     saying nothing at all. The toast is raised here rather than in the
 *     calling component because the timeout path (`#retire`) has no caller to
 *     return to: nobody is awaiting it three seconds later.
 */

import { snapshot, setSetting, setTxSetting } from "./api.js";
import { toast } from "./ui/toasts.svelte.js";

/** How long an unconfirmed local value may keep overriding the device. */
const OPTIMISTIC_TIMEOUT = 3000;
/** How long a row keeps saying 已生效 after a confirmed write. */
const OK_LINGER = 1200;

/** Poll periods, in ms — see `setTempo`. Live meters need the fast one; a
 *  static settings list does not, and a hidden window needs none at all. */
export const TEMPO = { live: 250, calm: 1000, off: 0 };

const txKey = (tx, settingId) => `${tx}:${settingId}`;

/** One transmitter's noise-cancel state as a setting value, or null if it
 *  hasn't reported one yet. */
export function ncPower(tx) {
  if (tx?.nc_enabled == null) return null;
  return tx.nc_enabled ? "on" : "off";
}

/** Every transmitter's noise-cancel flag, skipping empty slots and any that
 *  haven't reported one yet. */
function ncFlags(txs) {
  return (txs ?? [])
    .filter(Boolean)
    .map((tx) => tx.nc_enabled)
    .filter((v) => typeof v === "boolean");
}

/** One receiver-level value for a non-empty set of per-transmitter flags.
 *  "mixed" is a real state the UI has to render, not an error — the two
 *  transmitters can genuinely disagree, since each has its own button. */
function combine(flags) {
  if (flags.every(Boolean)) return "on";
  if (flags.every((v) => !v)) return "off";
  return "mixed";
}

/** Carry forward fields a newer frame didn't include (v2 firmware splits
 *  identity/level across several periodic frame types). */
function mergeTx(prev, next) {
  if (!next) return null;
  if (!prev) return next;
  const out = { ...next };
  for (const k of [
    "serial", "firmware", "product_name", "voice_tone", "charging", "battery",
    "nc_enabled", "nc_mode", "low_cut", "mic_leds", "auto_off", "nc_button",
  ]) {
    out[k] = next[k] ?? prev[k];
  }
  return out;
}

function mergeRx(prev, next) {
  if (!next) return prev ?? null;
  if (!prev) return next;
  return { serial: next.serial ?? prev.serial, firmware: next.firmware ?? prev.firmware };
}

function mergeStatus(prev, next) {
  if (!next) return null;
  if (!prev || prev.model_id !== next.model_id) return next;
  const nextSettings = next.settings ?? {};
  const prevSettings = prev.settings ?? {};
  const emptyFrame = Object.keys(nextSettings).length === 0;
  return {
    ...next,
    nc_enabled:
      emptyFrame && Object.keys(prevSettings).length > 0 ? prev.nc_enabled : next.nc_enabled,
    rx: mergeRx(prev.rx, next.rx),
    tx: next.tx.map((tx, i) => mergeTx(prev.tx?.[i], tx)),
    settings: emptyFrame ? prevSettings : { ...prevSettings, ...nextSettings },
    protocol_version: next.protocol_version ?? prev.protocol_version,
    gain_dial: next.gain_dial ?? prev.gain_dial,
  };
}

class DeviceStore {
  snap = $state(null);
  selected = $state(null);
  /** Distinguishes "nothing picked yet" (auto-select) from "the user cleared
   *  it deliberately" (leave it cleared). */
  userDeselected = $state(false);
  /** The carried-forward status (see `mergeStatus`). Internal bookkeeping —
   *  everything outside reads `status`, which folds optimistic values in. */
  #stable = $state(null);

  /** id -> value shown before the device confirms it. */
  optimistic = $state({});
  /** "<tx>:<id>" -> value, for settings addressed at one transmitter. */
  optimisticTx = $state({});
  /** id -> "writing" | "ok" | "error". Absent means idle. */
  writes = $state({});
  /** id -> the message behind an "error" write state. */
  writeErrors = $state({});
  /** The last transport-level failure (the poll itself failing), or null.
   *  Separate from per-setting errors: this one isn't attributable to a row. */
  error = $state(null);

  #deadlines = new Map();
  #okTimers = new Map();
  /** id -> a thunk that re-issues the write exactly as it was first made, so
   *  the failure toast's 重试 works for a per-transmitter write too (which
   *  needs the slot index the row itself no longer has in hand). */
  #attempts = new Map();
  #inFlight = false;
  /** null until the first `setTempo`, so the very first call can never be
   *  mistaken for a no-op change and skip starting the poll. */
  #tempo = null;
  #timer = null;

  devices = $derived(this.snap?.devices ?? []);
  device = $derived(this.devices.find((d) => d.id === this.selected) ?? null);
  settings = $derived(this.snap?.settings ?? []);
  settingsById = $derived(Object.fromEntries(this.settings.map((s) => [s.id, s])));

  /** The setting groups the connected model actually declares, in
   *  first-seen order. The navigation is built from this, so a new model with
   *  a new group gets a section for free. */
  groups = $derived([...new Set(this.settings.map((s) => s.group))]);

  #raw = $derived(this.device ? (this.#stable ?? this.snap?.status ?? null) : null);

  /** Device status with per-transmitter optimistic values folded in. */
  status = $derived.by(() => {
    const raw = this.#raw;
    if (!raw) return null;
    return { ...raw, tx: raw.tx.map((tx, i) => this.#applyTx(tx, i)) };
  });

  /** Receiver-level setting values, with local not-yet-confirmed writes on
   *  top. `noise-cancel-power` is synthesized from the two transmitters, since
   *  the receiver reports no single value for it. */
  values = $derived.by(() => {
    const base = { ...(this.#raw?.settings ?? {}) };
    const flags = ncFlags(this.status?.tx);
    if (flags.length > 0) base["noise-cancel-power"] = combine(flags);
    return { ...base, ...this.optimistic };
  });

  /** True when a supported mic is on the bus but couldn't be opened —
   *  a missing udev rule on Linux, a missing WinUSB driver on Windows. */
  accessIssue = $derived(!!this.snap?.probe?.permission_issue && this.devices.length === 0);

  #applyTx(tx, index) {
    if (!tx) return tx;
    const next = { ...tx };
    const tone = this.optimisticTx[txKey(index, "voice-tone")];
    const power = this.optimisticTx[txKey(index, "noise-cancel-power")];
    const mode = this.optimisticTx[txKey(index, "noise-cancel")];
    if (tone !== undefined) next.voice_tone = tone;
    if (power !== undefined) next.nc_enabled = power === "on";
    if (mode !== undefined) next.nc_mode = mode;
    return next;
  }

  writeState(id) {
    return this.writes[id] ?? "idle";
  }

  // --- Polling ---------------------------------------------------------

  /** Switch poll rate. `TEMPO.off` stops it entirely (window hidden). */
  setTempo(ms) {
    if (this.#tempo === ms) return;
    this.#tempo = ms;
    if (this.#timer) clearInterval(this.#timer);
    this.#timer = null;
    if (ms > 0) {
      this.refresh();
      this.#timer = setInterval(() => this.refresh(), ms);
    }
  }

  stop() {
    if (this.#timer) clearInterval(this.#timer);
    this.#timer = null;
    this.#tempo = TEMPO.off;
  }

  async refresh() {
    if (this.#inFlight) return;
    this.#inFlight = true;
    try {
      const next = await snapshot(this.selected);
      this.snap = next;

      if (!this.selected && !this.userDeselected && next.devices.length > 0) {
        this.selected = next.devices[0].id;
      }
      if (this.selected && !next.devices.some((d) => d.id === this.selected)) {
        this.selected = next.devices[0]?.id ?? null;
        this.#forget();
      }
      this.#stable = next.status ? mergeStatus(this.#stable, next.status) : null;
      this.#retire(next.status);
      this.error = null;
    } catch (e) {
      this.error = String(e);
    } finally {
      this.#inFlight = false;
    }
  }

  /** Drop optimistic values the device has now confirmed — or that have run
   *  out of time waiting for it to. */
  #retire(status) {
    const confirmed = status?.settings ?? {};
    const txs = (status?.tx ?? []).filter(Boolean);
    const flags = ncFlags(txs);
    const now = Date.now();

    const keep = {};
    for (const [id, want] of Object.entries(this.optimistic)) {
      // `noise-cancel-power` has no receiver-level truth: it's confirmed only
      // when every transmitter reports the value we asked for.
      const settled =
        id === "noise-cancel-power" && flags.length > 0
          ? combine(flags) === want
          : confirmed[id] === want;
      if (settled) {
        this.#settle(id, "ok");
      } else if (now > (this.#deadlines.get(id) ?? Infinity)) {
        this.#fail(id, "设备未在预期时间内确认这次修改");
      } else {
        keep[id] = want;
      }
    }
    if (Object.keys(keep).length !== Object.keys(this.optimistic).length) this.optimistic = keep;

    const keepTx = {};
    for (const [key, want] of Object.entries(this.optimisticTx)) {
      const [txText, id] = key.split(":");
      // The receiver mirrors NC power/mode into both transmitter records even
      // after a targeted write, so those can never be confirmed per-slot —
      // they stay as session truth. Only Voice Tone reads back independently.
      const settled = id === "voice-tone" && txs[Number(txText)]?.voice_tone === want;
      if (settled) this.#settle(id, "ok");
      else keepTx[key] = want;
    }
    if (Object.keys(keepTx).length !== Object.keys(this.optimisticTx).length) {
      this.optimisticTx = keepTx;
    }
  }

  #settle(id, state) {
    this.#deadlines.delete(id);
    this.writes = { ...this.writes, [id]: state };
    clearTimeout(this.#okTimers.get(id));
    this.#okTimers.set(
      id,
      setTimeout(() => {
        const { [id]: _drop, ...rest } = this.writes;
        this.writes = rest;
      }, OK_LINGER),
    );
  }

  #fail(id, message) {
    this.#deadlines.delete(id);
    this.writes = { ...this.writes, [id]: "error" };
    this.writeErrors = { ...this.writeErrors, [id]: message };
    if (id in this.optimistic) {
      const { [id]: _drop, ...rest } = this.optimistic;
      this.optimistic = rest;
    }
    const again = this.#attempts.get(id);
    toast.error(`“${this.settingsById[id]?.label ?? id}”未生效`, {
      detail: message,
      action: again && { label: "重试", run: () => again() },
    });
  }

  // --- Writes ----------------------------------------------------------

  /** Drop everything that described the previously selected device.
   *
   *  Per-transmitter noise-cancel values are deliberately never retired by a
   *  poll (the receiver mirrors them into both slots, so a frame can't confirm
   *  one slot alone) — which means they'd otherwise survive a device swap and
   *  describe the new receiver's transmitters using the old one's state. */
  #forget() {
    this.#stable = null;
    this.optimistic = {};
    this.optimisticTx = {};
    this.writes = {};
    this.writeErrors = {};
    this.#deadlines.clear();
    // Retrying against a device that is no longer selected would write to the
    // wrong receiver, so the pending thunks go with everything else.
    this.#attempts.clear();
  }

  select(id) {
    if (id !== this.selected) this.#forget();
    this.selected = id;
    this.userDeselected = id === null;
    this.refresh();
  }

  /** Write a receiver-level setting. Resolves to an error message, or null. */
  async change(id, value) {
    if (!this.selected) return "尚未选择设备";
    // A broadcast write supersedes any per-transmitter override of the same
    // setting; leaving those in place would keep showing the old split.
    if (id === "noise-cancel-power" || id === "noise-cancel") {
      this.optimisticTx = Object.fromEntries(
        Object.entries(this.optimisticTx).filter(([k]) => !k.endsWith(`:${id}`)),
      );
    }
    this.optimistic = { ...this.optimistic, [id]: value };
    this.writes = { ...this.writes, [id]: "writing" };
    this.#deadlines.set(id, Date.now() + OPTIMISTIC_TIMEOUT);
    this.#attempts.set(id, () => this.change(id, value));
    try {
      await setSetting(this.selected, id, value);
      await this.refresh();
      return null;
    } catch (e) {
      this.#fail(id, String(e));
      return String(e);
    }
  }

  /** Write a setting on one transmitter slot. */
  async changeTx(tx, id, value) {
    if (!this.selected) return "尚未选择设备";
    const key = txKey(tx, id);
    const next = { ...this.optimisticTx };
    if (id === "noise-cancel-power" || id === "noise-cancel") {
      // Snapshot every visible slot before the targeted write: the next status
      // frame mirrors the new value into both records, so the untouched
      // transmitter's real value can't be recovered from it afterwards.
      for (const [i, item] of (this.status?.tx ?? []).entries()) {
        if (!item || next[txKey(i, id)] !== undefined) continue;
        const current = id === "noise-cancel" ? item.nc_mode : ncPower(item);
        if (current !== undefined && current !== null) next[txKey(i, id)] = current;
      }
    }
    this.optimisticTx = { ...next, [key]: value };
    this.writes = { ...this.writes, [id]: "writing" };
    this.#deadlines.set(id, Date.now() + OPTIMISTIC_TIMEOUT);
    this.#attempts.set(id, () => this.changeTx(tx, id, value));
    try {
      await setTxSetting(this.selected, tx, id, value);
      await this.refresh();
      // NC power/mode can't be confirmed per-slot (see `#retire`), so a
      // successful call is the only completion signal those two ever get.
      if (id !== "voice-tone") this.#settle(id, "ok");
      return null;
    } catch (e) {
      const { [key]: _drop, ...rest } = this.optimisticTx;
      this.optimisticTx = rest;
      this.#fail(id, String(e));
      return String(e);
    }
  }

  // --- Locking ---------------------------------------------------------

  /** Why `setting` can't be changed right now, or null if it can. */
  lockReason(setting) {
    const status = this.status;
    const values = this.values;
    const label = (id) => this.settingsById[id]?.label ?? id;

    // Settings v2 firmware introduced don't exist on a v1 device at all.
    if (status?.protocol_version === 1 && setting.v1_command == null) {
      return "需要 v2 协议，请更新固件";
    }
    // NC mode is meaningless while NC is off. On v1 firmware there's no
    // software toggle for it at all — only the transmitter's own button.
    if (setting.id === "noise-cancel" && values["noise-cancel-power"] === "off") {
      return this.settingsById["noise-cancel-power"]
        ? `请先开启“${label("noise-cancel-power")}”`
        : "请先用发射器按键开启降噪";
    }
    for (const other of setting.exclusive_with ?? []) {
      // Audio Channels is an enum, so its "active" value isn't the usual "on".
      if (other === "stereo") {
        if (values.stereo === "stereo") return `请先将“${label("stereo")}”切换为单声道`;
      } else if (values[other] === "on") {
        return `请先关闭“${label(other)}”`;
      }
    }
    return null;
  }
}

export const devices = new DeviceStore();
