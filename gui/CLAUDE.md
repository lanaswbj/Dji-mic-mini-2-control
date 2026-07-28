# gui/

The shipped product: Tauri 2 backend (`src-tauri`) + Svelte 5 frontend (`src`). Build and dev commands
are in the root CLAUDE.md.

## Backend (`src-tauri`)

`main.rs` wires up the Tauri app — single-instance, autostart, close-to-tray (hides instead of
quitting; the tray "quit" item is the real exit path), and a tray icon reflecting live device/battery
state. Composited into its own top-left corner (the opposite corner from the device badges — the two
aren't mutually exclusive) is a coarse Claude Code idle/thinking/working/error/attention status from
`claude_status.rs`, a tiny OS-agnostic atomic updated by hook events relayed from `hook_bridge.rs`.
That status is deliberately last-write-wins rather than a precise per-session state machine, because
Claude Code's hooks are fire-and-forget with no request/response correlation.

`commands.rs` holds the general snapshot/set-setting commands plus `app_info`/`set_autostart` for the
偏好设置 screen. `app_info` reads the version, autostart state, `pie_menu::HOTKEY_LABEL` and the two
loopback `PORT` constants **from the modules that own them** rather than letting the frontend restate
them; `pie_menu::pie_menu_slots` does the same for the six fixed slot descriptions, declared right
beside the `match index` in `pie_menu_select` that implements them, so the UI cannot describe a slot as
doing something it doesn't.

Windows-only concerns get their own modules:

- **`driver.rs`** — one-click WinUSB driver install for the vendor control interface. A hand-rolled INF
  can't install under Code Integrity enforcement without a signed catalog, so this downloads the
  official signed Zadig release, verifies its Authenticode signature, pre-seeds `zadig.ini` so it opens
  in the right state, and deletes it afterward. It does not try to reimplement Zadig's catalog-signing.

- **`pairing_button.rs`** — reads the pairing button via the Win32 Raw Input API (`RIDEV_INPUTSINK`).
  `RIDEV_NOLEGACY` was tried to also suppress Windows' own default handling of that HID collection (an
  automatic system-volume change on every press) but is only valid for the Generic Desktop usage page
  (mouse/keyboard) — using it on the Consumer Control page makes `RegisterRawInputDevices` fail
  outright, **silently**, since the original code didn't check the return value. That meant detection
  itself was broken too, not just the volume suppression; both bugs shipped together undetected for a
  while. A `WH_KEYBOARD_LL` hook doesn't work for the suppression either — Windows' default handling
  for HID consumer-page usages happens below the synthesized-keystroke layer. The volume side effect is
  therefore handled after the fact by `volume_guard.rs`, not solved at the input level.
  Every press also unconditionally simulates an Enter keypress (via `key_inject.rs`) as a
  general-purpose "pairing button = Enter" remap — that is what lets the pairing button confirm the pie
  menu without `pie_menu.rs` needing to know the button exists.

- **`volume_guard.rs`** — neutralizes that volume side effect: continuously snapshots the default
  output device's volume/mute state while idle and forces it back shortly after every press, and
  separately keeps the volume OSD popup hidden for as long as the app runs. (A press-triggered
  one-shot version of the OSD suppression was tried first but still flashed occasionally, for reasons
  never pinned down — see the root CLAUDE.md's open items.)

- **`mic_tap.rs`** — detects 1–2 taps on the mic shell as an audio-domain gesture (3+ in a burst still
  report as a double tap). Feeds per-chunk features into the trained classifier (see
  `crates/tap-model/CLAUDE.md`), gated by `earshot` VAD so a sharp consonant or loud exclamation
  mid-speech doesn't slip through. `MicTapWatcher` holds the live `Arc<TapModelStore>` (swapped by a
  background thread watching `tap_model.json`'s mtime — `spawn_model_poll` — *and* by
  `tap_feedback.rs`'s incremental trainer, from two entirely different threads, with no locking on the
  audio-thread read path), an `Arc<tap_feedback::FeedbackRing>` (every chunk's raw measurements, pushed
  unconditionally *before* any suppression/floor branching), and `last_group_taps` (confirm instants of
  the most recently finalized group, for false-positive targeting). Hard amplitude/ratio/crest floors
  sit on top of the model as a last-resort safety net, plus a short confirm-delay/decay-check
  (`TAP_CONFIRM_DELAY`/`SUSTAIN_DECAY_FRACTION`) that rejects continuous noise (blowing, rubbing) which
  a single chunk's snapshot could otherwise mistake for an impact. The module doc comment has the full
  per-chunk pipeline order.

- **`tap_feedback.rs`** — incremental training driven by in-app user feedback, built on `mic_tap.rs`'s
  `FeedbackRing`. Two Tauri commands (`mic_tap_report_false_positive`/`…_false_negative`, wired to
  buttons in `src/lib/sections/InputGestures.svelte`) locate the actual acoustic event in the ring
  buffer — a false positive looks backward from the confirmed group's instants minus
  `TAP_CONFIRM_DELAY`; a false negative scans the last few seconds for the loudest chunk above a
  relaxed sanity floor — append it correctly labeled to a per-user CSV (`tap_feedback.csv`, same schema
  as detect-test's `samples.csv` so a future full retrain can fold it back in), then kick off a bounded
  warm-started retrain (`tap_model::continue_training`) on a background thread: a handful of low-LR
  epochs from the *live* model's weights, using the new rows (up-weighted by repetition) plus a bulk
  "background" replay sample drawn straight from whatever is currently in the ring buffer — real,
  this-room, this-hardware ambient audio, deliberately not a bundled dataset, so nothing extra has to
  ship. Before touching the live model the candidate must have all-finite weights **and** must not
  regress "none"-class accuracy on that same replay sample past a small tolerance (a cheap proxy for
  "did this just make false triggers on ordinary noise more likely"). A rejected candidate is
  discarded, but its CSV rows remain for a later attempt or a full offline retrain.
  `mic_tap_training_status` (polled by the UI) reports model source/age/row-count/pending-feedback
  count; `mic_tap_rollback_model`/`mic_tap_restore_factory_model` restore the pre-update backup
  (`tap_model.json.bak`) or the embedded baseline, each backing up the about-to-be-replaced file first,
  so "restore factory" is itself undoable.

- **`shortcut.rs`** — permanent stub, always unavailable on Windows. See the root CLAUDE.md.

- **`pie_menu.rs`** — a global Ctrl+Alt+P hotkey (`RegisterHotKey`; an earlier version bound bare `K`,
  which made that letter untypeable system-wide, before settling on a combo with no known collision)
  toggling a borderless, transparent, always-on-top overlay docked above the taskbar: an LG
  webOS-style fan/pie menu. Arrow keys (or a mic tap) move the highlight, Enter (or the pairing button)
  confirms, Escape or losing focus cancels. Like `pairing_button.rs`, the hotkey is registered on a
  hidden message-only window with its own `GetMessageW` loop on a dedicated thread; `WM_HOTKEY` hops
  back onto the Tauri main thread via `run_on_main_thread` before touching the window, since
  window/webview calls aren't meant to be made from arbitrary threads. The overlay is created once at
  startup and only shown/hidden/repositioned afterward, never rebuilt per toggle.
  The six fixed slots (`pie_menu_select`): voice-dictation hold (Win+Ctrl, held until a later
  pairing-button press ends it), Down, Up, Enter, types `"/btw "`, close. Beyond that,
  `permission_server.rs` can override what's showing with a pending Claude Code question — a
  `PermissionRequest` choice or a single-select `AskUserQuestion` — with real title/option text in a
  panel above the arc; an `AskUserQuestion`'s implicit freeform "Other" reuses the voice-dictation slot
  instead of a keypress. Fan geometry lives in `src/PieMenu.svelte` — a half-circle "dome", items
  placed along the upper arc by simple trig, with the question panel's `PIVOT_X`/`PIVOT_Y` split from
  the arc's own radius so a panel can grow without moving the arc.

- **`key_inject.rs`** — `SendInput`-based keystroke simulation, so a pie-menu slot acts on whatever
  application currently has focus, the same mechanism a hardware keyboard uses. `hold_win_ctrl_start`/
  `hold_win_ctrl_end` are separate key-down-only/key-up-only calls rather than one press-and-release
  function, because the Win+Ctrl voice-dictation hold spans an arbitrarily long gap between two
  unrelated events (the slot that starts it, a later pairing-button press that ends it). `type_text`
  uses Unicode key events rather than virtual-key codes so it isn't limited to characters with a
  virtual key on the current keyboard layout.

- **`hook_bridge.rs`** — a loopback-only TCP listener (`127.0.0.1:47215`) bridging Claude Code's
  *non-permission* lifecycle hook events (`PreToolUse`/`PostToolUse`/`Stop`/etc.) into the pie menu and
  `claude_status.rs`. `~/.claude/settings.json` (outside this repo) registers a `"command"`-type hook
  per event that reads the event JSON off stdin and forwards it unmodified via a PowerShell one-liner
  reading stdin as explicit UTF-8, so non-ASCII payloads survive. Two jobs: (1) an `AskUserQuestion`'s
  `PostToolUse` event auto-dismisses a stale pie-menu question overlay if it was answered some other
  way (typed in the terminal, or `permission_server.rs`'s own "answer in terminal instead" escape
  hatch) — a no-op if the pie menu already cleared its own pending state; (2) every event with a
  `hook_event_name` updates the coarse tray-icon status. `HookPayload`'s fields are all `Option<T>`
  with no `deny_unknown_fields`, so one struct parses every event type without one event's payload
  shape breaking another's. **Does not handle permission decisions at all anymore.**

- **`permission_server.rs`** — a loopback-only **HTTP** server (`127.0.0.1:47216/permission`, one port
  up from `hook_bridge.rs` so the two never collide) registered in `~/.claude/settings.json` as an
  `"http"`-type `PermissionRequest` hook. This is the real allow/deny decision channel, not a
  notification relay. An earlier iteration of this project believed the `"http"` hook type never
  actually delivered real `PermissionRequest` events, and built `hook_bridge.rs`'s keystroke-simulation
  fallback specifically to work around that; a later from-scratch probe proved that belief **wrong** (or
  a newer Claude Code fixed it). The `"http"` hook genuinely works, and a real decision response is
  honored with zero terminal interaction. Key points, since this reverses what older notes said:
  - `AskUserQuestion` fires this exact same `PermissionRequest` hook (confirmed empirically, documented
    nowhere) rather than some separate mechanism — a plain tool needing Allow/Deny and an interactive
    multi-choice question arrive at the identical endpoint, distinguished only by
    `tool_name == "AskUserQuestion"`.
  - The response body must be the `{"hookSpecificOutput":{"hookEventName":"PermissionRequest",
    "decision":{"behavior":"allow"|"deny",...}}}` envelope — only that shape's `updatedInput` field
    lets an `AskUserQuestion` answer be supplied directly (`answers: {question_text: chosen_label}`)
    instead of falling back to Claude Code's terminal picker. That is what makes answering from the pie
    menu possible with **no keystroke injection and no focus stealing at all**.
  - `QUEUE` (a `Mutex<VecDeque<PendingRequest>>`, only the front item ever shown via `show_front`)
    exists because requests routinely overlap in practice — a Claude Code session working on this repo
    is itself a constant client of this server, so a human's separate session can easily have its own
    request in flight simultaneously. An earlier single-`Option`-slot design let a second arrival
    silently overwrite the first, surfacing as two real bugs at once: "Allow" appearing to do nothing
    (the visible card's request had already been denied out from under it), and the overlay
    flashing/re-focusing on every new arrival even when one was already shown.
  - **No `permission_mode`-based gating.** An earlier version auto-allowed everything except `default`
    mode, on the theory that only `default` needs a human decision; real use falsified that — a Bash
    command in `acceptEdits` mode (which only auto-accepts *file edits*) still fired this hook
    expecting a real answer. Every request now pops the pie menu and waits, full stop: Claude Code
    firing the hook at all already means it wants an answer.
  - "Allow, don't ask again" persists matching `permission_suggestions` rules into
    `{cwd}/.claude/settings.local.json`'s `permissions.allow` array — the same file/format Claude
    Code's own `destination: "localSettings"` suggestions target. Best-effort: a failed
    read/merge/write still lets the already-decided allow proceed, just without being remembered.
- **`claude_hooks.rs`** — writes the two hooks above into the *user-level* `~/.claude/settings.json`,
  which is what turns them from two open sockets into a working feature. (Not to be confused with
  `permission_server`'s own writes, which target the *project-level*
  `{cwd}/.claude/settings.local.json`; different file, different purpose.) Three rules it exists to
  enforce, all of them load-bearing because the file is not ours and routinely carries other tools'
  hooks: **never clobber** (parsed as a generic `Value` and mutated in place, anything unrecognised
  copied through), **be idempotent** — entries are recognised by the *port number* in the
  command/url rather than by a marker field, precisely so a hand-written registration (which is what
  every existing user has) is adopted instead of duplicated — and **be reversible** (the first write
  leaves a `.djimic-backup` beside the file; uninstall removes exactly what install added and cleans
  up emptied groups). A `settings.json` that exists but does not parse as an object is the one state
  it refuses to touch at all.

## Frontend (`src`)

Svelte 5 (runes), poll-based throughout. There is no Tauri event emission anywhere in the codebase
*except* `pie-menu:open`, which `pie_menu.rs` emits to reset the overlay's selection each time it is
shown. `main.js` mounts one of two root components depending on `getCurrentWindow().label`: `App` for
`"main"`, `PieMenu` for `"pie-menu"` — **both windows load the same bundle**, so anything window-scoped
(the appearance preference, for one) has to be applied by *both* roots. `lib/api.js` is the thin
`invoke()` wrapper layer; add new Tauri commands there rather than calling `invoke` directly from a
component. All copy is Simplified Chinese.

The main window is a **section-based app navigated from a floating dock**, not a tabbed one. See
`design-system/dji-mic-control/MASTER.md` for the design system that `app.css` and `lib/ui/`
implement, including a table mapping each of its sections to the code implementing it.

The window is **transparent with a Windows 11 Acrylic backdrop** (`transparent: true` +
`windowEffects` in `tauri.conf.json`), so `body` is `background: transparent`. Four things about it
have each already cost a real defect — the full reasoning is §2.4 of the design system, this is the
short form:

- **Acrylic, not Mica.** Mica samples only the wallpaper and deliberately shows nothing of the
  windows behind, so at any legible alpha it is invisible. Don't "restore" it.
- **The gutter is unpainted — but only while there is a backdrop to show.** The `--panel-gap` strip
  around the floating content plane is the only region no card can cover, and therefore the only place
  the material is unambiguously visible; painting it kills the effect at the one spot there is nothing
  else to look at. **The moment the effect is off it stops being a material and becomes a hole**: the
  window is `transparent: true`, so with the backdrop gone that strip shows the desktop straight
  through and the app reads as a screenshot with its edges cut out. Flattening the `--glass-*` alphas
  does not reach it, because the gutter paints no material at all — it is `body`. Hence
  **`--window-bg`**, flipped in the same three places as the alphas (`data-translucent="off"`,
  `prefers-reduced-transparency`, `prefers-contrast`). Anything else that relies on the window being
  see-through needs the same treatment; "glass off" means three signals, not one.
- **The title bar is not part of that.** It is opaque `--surface` (pure white in the light theme),
  by request, and ignores the 外观 → 窗口毛玻璃 switch. It used to be unpainted like the gutter and
  read as a *hole*: a band of desktop above the app with the app's own name floating in it and no
  surface under the buttons. It is the one surface whose job is to not be translucent, so it takes no
  `--glass-*` alpha at all.
- **The content plane is the only glass layer; cards are opaque.** Three arrangements have shipped
  and the distinction between them is the whole point — do not collapse them:
  1. Opaque cards over a **thick** plane (0.48). Cards cover nearly the whole content area, so the
     backdrop only showed in the gaps between them: translucent on paper, invisible in fact.
  2. Translucent cards (0.62) over that same plane. Body text then sat on the sum of two alphas
     (~0.80 effective), which reads as a washed-out panel rather than a material — and it inverted
     the platform convention, leaving the content see-through and the chrome solid.
  3. **Current:** opaque cards (`--glass-card: 1`) over a **thin** plane (`--glass-content` 0.30
     light / 0.34 dark). Nothing has to be legible on the plane any more — no body text sits
     directly on it, `SectionHeader` carries its own `--material-chrome` — so the alpha went *down*
     rather than up. The material is now continuous across the whole content area instead of
     surviving only in card gaps, card text contrast is an exact theme-independent number again, and
     the step from the gutter to the plane is small enough that the gutter stops reading as a
     cut-out. The ladder is monotonic in one direction: gutter (undiluted acrylic) → plane → card.
  Cards also drop `--glass-gloss` (a gloss keeps a *translucent* rectangle from reading flat; on an
  opaque surface it is a white streak) but keep `--glass-sheen`, which is legitimate elevation
  either way. And **no `backdrop-filter` on the plane** — see the point below: nothing is painted
  beneath it for one to sample, so it would be pure GPU cost.
- **`backdrop-filter` cannot see the OS backdrop.** It samples what the *page* painted below the
  element, so it belongs only where in-page content genuinely scrolls under something: the section
  header, **the dock**, popovers, toasts, dialogs. On the title bar and the old sidebar it blurred an
  empty rectangle. The dock is the clearest legitimate case in the app: the content plane genuinely
  scrolls underneath it.

`lib/glass.svelte.js` owns the on/off preference, and it is now **pure CSS** — it calls no Tauri API
at all. The backdrop is applied once by `windowEffects` in `tauri.conf.json`, at window creation, and
is never removed; "off" makes the page opaque instead, which hides it exactly as completely.

That is a deliberate retreat from an IPC design that broke twice, in opposite directions:

- **Turning it back on did not visibly work.** `setEffects` writes
  `DWMWA_SYSTEMBACKDROP_TYPE` correctly, but DWM will not recompose an *already visible* window's
  frame just because that attribute changed — the glass only appeared after the user minimised and
  restored the window (or resized it). Removing the toggle's OS call removed that path entirely.
- **Before that, every call was being refused** for a missing permission, and the code treated *any*
  rejection as "the backdrop is gone" and flattened the CSS — switching off a correctly configured,
  actively rendering Acrylic from the frontend and making it read as "the glass was never
  implemented".

**The same DWM quirk bites at startup, and that half lives in `main.rs::reveal_backdrop`.** The
window is created with `"visible": false` and revealed later (so autostart comes straight up into the
tray), so the backdrop attribute lands on a window DWM has never composed — and *every launch* came
up opaque until the user minimised and restored it. `reveal_backdrop` re-applies the effect and then
forces the recalculation with `SetWindowPos(…, SWP_FRAMECHANGED | NOMOVE | NOSIZE | NOZORDER |
NOACTIVATE)`, on the two paths that reveal the window (startup, and `show_main` from the tray). It
does both because "attribute ignored on a hidden window" and "attribute stored but never composed"
are indistinguishable from that side. **Anything that makes this window visible needs it** — a new
reveal path that skips it will reintroduce the exact same bug report.

Two facts from that era are still worth not rediscovering: `core:default` grants **read-only** window
getters, so any mutation (`set_effects`, `minimize`, `start_dragging`, …) needs its own explicit
entry; and there is **no** `core:window:allow-clear-effects` — `clearEffects()` is not a second
command, it invokes the same `plugin:window|set_effects` with `value: null`. Inventing the symmetric
name fails the *build* with `Permission … not found`, because capability names are validated at
compile time by the build script. Which is also why **`npm run build` can never verify a change under
`src-tauri/`, `capabilities/` included** — use `cargo check -p djimic-gui`.

- **`lib/nav.js`** — the navigation model. Two tiers: **设备** (概览 → one section per protocol setting
  *group* → 设备信息) and **应用** (敲击与按键 / 快捷菜单 / 偏好设置). The middle of the 设备 tier is
  data-driven from `Setting.group`, so a new model declaring a new group gets a section without anyone
  editing the frontend. The two tiers are flattened into one list for the dock; `Ctrl+1..9` jumps to a
  section, `Ctrl+,` opens preferences, `Ctrl+R` forces a refresh. (`Ctrl+B` is gone with the sidebar.)
- **`App.svelte`'s shell** — an opaque title bar, one full-width content plane inset by `--panel-gap`,
  and the navigation floating over it as `lib/Dock.svelte`. The 236px sidebar this replaced was
  spending a fixed quarter of the window on seven labels that never change, and forced every section
  to lay itself out against a column whose width depended on a toggle. The receiver picker moved into
  the title bar (`DeviceSwitcher compact`): a scope selector belongs in chrome, and that is the only
  chrome left.
- **`lib/Dock.svelte`** — uniform 44px squares, labels in tooltips, one measured pill that slides
  between them. **Deliberately not reorderable.** The first version shipped drag-to-reorder here and
  it was the wrong control for it: every press on a nav item then has to be disambiguated from the
  start of a drag, a tax paid on every click to buy a rearrangement nobody performs twice. Reordering
  lives on the pie menu's slots instead (`lib/pieOrder.svelte.js`), where the order genuinely decides
  how far the selection has to travel. `Section.svelte`'s bottom padding is `--dock-clear` (set on
  `.content`) so the last card can scroll clear of the dock rather than sitting permanently half
  covered.
- **The flex-`gap` trap**, kept because it is invisible in the markup and cost a "the whole thing
  looks subtly crooked" bug report on the old rail: **a flex `gap` survives a zero-width child.**
  Sending a label to `width: 0` leaves the item's 12px gap in place, so what gets centred is
  glyph-plus-gap and every icon sits exactly half a gap — 6px — left of true centre, uniformly.
  Anything that centres by removing siblings must zero the `gap` and the inline padding too.
- **`lib/fluidScroll.js`** — the Svelte action on `.content` supplying a rubber band at each end
  (Chromium's own elastic overscroll is off via `overscroll-behavior: none`). **There is no gesture
  phase and no release phase**, and every version that had them was wrong. Chromium runs its own fling
  animation for a precision touchpad, so wheel events keep arriving for up to a second after the
  fingers have left the pad — a design that waits for silence before springing waits out the whole
  fling, and the fling's tail is uneven, so every gap longer than the grace window started the spring
  and every event after it cancelled and re-extended it. Two or three of those in a row is precisely
  the reported "卡一下、又卡两下、然后才弹回来", and no grace-window value fixes it, because the
  silence it waits for never arrives while the user is still watching. The spring now integrates
  toward zero on *every* frame and a wheel event only displaces it: sustained pushing reaches an
  equilibrium (the band sits out under pressure, which is what it should look like), and the instant
  the input weakens the spring is already winning. Consequences worth not undoing — no velocity is
  estimated from event timestamps (there is no handoff to seam, and those estimates were themselves a
  jitter source: an event 4ms after its predecessor implies ten times the real velocity); resistance
  is applied on *intake*, so the stored excursion is the painted excursion and the spring's numbers
  mean what they say; and there is still no axis latch, since `overflow-x: hidden` already makes the
  container unable to move sideways and the old JS latch swallowed whole gestures when a flick began
  with a few px of drift. **It is now the container's only wheel owner** — every same-axis event is
  `preventDefault`ed, not just the ones pushing against an end stop, because the mid-range is where
  Chromium's own eased wheel animation lives and that easing is the "acceleration" the module exists
  to remove. So it drives `scrollTop` for the whole range, at a **constant px/s** toward a `target`
  a wheel event merely displaces: displacement over time is a straight line, no ease in or out.
  `speed` is recomputed **only when input arrives**, never per frame — per-frame recomputation from
  the remaining distance *is* exponential decay, i.e. exactly the easing being removed. A precision
  touchpad still coasts, and cannot be stopped from coasting: Windows' driver keeps sending real
  wheel events for up to a second after the fingers leave the pad, and they are applied 1:1.
  Two consequences of owning both jobs in one module: a band may only *start* once the target is
  pinned at an end **and** the viewport has caught up to it (otherwise a fast flick bounces while
  still coasting toward the bottom — an excursion nowhere near an end, swallowing every event
  after it), and `adopt()` has to notice scroll positions the module did not set, since `go()` runs
  `scrollTo({top: 0})` on every navigation and the arrow keys scroll natively.
  **The excursion is also published as `--bounce` on the
  container, and `SectionHeader` cancels it with an equal `translateY`.** Translating the scroll
  container translates everything in it, sticky header included — so pulling past the top slid the
  screen's own title down and opened an empty band above it, which reads as the whole page having
  come loose rather than as content hitting its end. Pinning the chrome and letting only the cards
  travel under it is what the header is built for and what the same gesture does on iOS. An inherited
  custom property rather than a prop: the header is three components down, and re-rendering it 60
  times a second to move it is the wrong mechanism when the compositor can do it with no JS at all.
  **The invariant that matters most: an excursion may only exist at a real end stop.** Every wheel
  event is `preventDefault`ed while one is live, so a band that survives into the middle of the range
  presents as "the page won't scroll and something is bouncing around" — the worst failure this
  module has, and the one that was actually shipped. Two things guard it now. A reversal that
  cancels the excursion **must zero the spring's velocity too**: the spring is normally pulling
  *inward* when the user reverses, so zeroing the position alone launched the band straight out the
  opposite side, self-sustaining and nowhere near an end. And `tick` re-checks `anchored()` every
  frame and drops anything that no longer holds, because plenty of ordinary things strand a band —
  navigating a section runs `scrollTo({top: 0})`, and 概览's live meters change the content height
  four times a second.
- **`lib/pieOrder.svelte.js`** — the pie menu's slot order, the one list in the app that is
  user-rearrangeable (edited from 快捷菜单 in the main window, never on the overlay itself). Entries
  are **stable slot indices** — `pie_menu.rs`'s `SLOTS[i].index`, which is what `pie_menu_select`'s
  `match` is written against — so reordering never changes what a slot does, and `PieMenu.svelte`
  maps the picked screen position back through the order before invoking. The backend has no notion
  of order at all. Two invariants are enforced on every read, and a stored value violating either is
  discarded whole rather than repaired: the **close slot stays last** (`PieMenu.svelte` recognises
  close as `ITEMS.length - 1`, and a question overlay reuses the same convention), and the value must
  be a **full permutation** (a short one would silently hide an action). It lives in localStorage
  rather than in the backend because the overlay and the main window are two WebViews of one origin
  and share it; the overlay calls `refresh()` on each `pie-menu:open`, which is the only moment the
  order can matter.
- **`lib/store.svelte.js`** — the single owner of device state (`devices`, a rune-based class): the
  snapshot poll, the v2 carry-forward merge (`mergeStatus`/`mergeTx`/`mergeRx`), optimistic writes, and
  `lockReason`. Four behaviors matter, each fixing a real defect in the pre-redesign build:
  - The poll **pauses when the window is hidden** — closing to tray used to leave a 250ms USB poll
    running forever. Otherwise it runs at `TEMPO.live` (250ms, only while 概览 with its live meters is
    showing) or `TEMPO.calm` (1s).
  - Every optimistic value has a **3s deadline**, after which it reverts rather than leaving the UI
    permanently showing a value the hardware doesn't have.
  - `writeState(id)` exposes idle/writing/ok/error so a row can show a write in flight, and every
    failure also raises a toast carrying the reason plus a retry. The toast is raised in the store, not
    the calling component, because the timeout path has no caller to return to — nobody is awaiting it
    three seconds later.
  - Per-transmitter noise-cancel values are **deliberately never retired by a poll**: the receiver
    mirrors them into both slots, so a frame can't confirm one slot alone. That is exactly why
    `#forget()` has to clear them on a device swap — otherwise they'd describe the new receiver's
    transmitters using the old one's state.
- **`lib/ui/`** — the shared primitives, and the only place a control's markup and CSS may live.
  `Icon`'s `<script module>` holds the app's only icon table; there are no emoji anywhere in the UI.
  Errors are toasts, never in-flow banners — a banner reflowed the whole page at the worst possible
  moment. No component may hardcode a color, spacing, radius, duration or font size; every one comes
  from a custom property in `app.css`. Three traps here have each already cost a real defect:
  - `Icon`'s svg is `display: inline-block`, not `block`. A block-level SVG dropped into an inline
    context takes its own line, which is what put 概览's「全部音频设置」chevron on a second row. A
    trailing glyph on a `Button` goes through `iconEnd`, never into `children`.
  - Responsive rules are **container queries** against `.content` (`@container content (…)`), not
    viewport media queries. The reading column is never the window width — it is capped at
    `--measure-wide` and, back when a sidebar could be open, was ~520px at the 760px minimum window
    width. `@media (max-width: 640px)` cannot see either, which is exactly how `Row`'s stacking
    breakpoint failed to fire at the size it existed for.
  - An icon leading text that can wrap uses `.u-icon-line` (app.css). Four components had each
    grown an `align-items: center` copy, correct on one line and visibly wrong on two.
  - **A wrappable flex row's text column needs `flex: 1 1 0`, not `1 1 auto`.** Line breaking uses
    each item's *hypothetical* main size, and an `auto` basis means max-content — so `Row`'s text
    column demanded the full label width, wrapped itself onto line two, and stranded the subject
    glyph alone on line one with the label restarting underneath it. `min-width: 0` does not save
    you: shrinking happens only after the line has already been broken. This is what "把窗口拉窄，
    里面的文字排版会完全乱掉" was. Related: `Row`'s wrap rule now puts a `min-width` floor on that
    column instead of forcing `.right` to `flex-basis: 100%`, so the control drops to a second line
    only when it genuinely does not fit — 概览's two-up transmitter cards sit permanently under the
    breakpoint and were spending a whole extra line on a 44px switch.

  Sizes are tokens too, not just colour and space: `--control-h` (32px — Button/Switch/Segmented/
  swatch rows, anything you *set a value with*) and `--hit` (44px — dock items, probes, picto
  plates, anything you *aim at*). `--dock-clear` in `App.svelte` is computed from `--hit` rather
  than hand-summed, because it was a `100px` silently tied to the dock item's size. Both
  `<dialog>`s take their scrim (`--blur-scrim`, which switches off with the other blurs) and their
  entrance animation from app.css's bare `dialog` rules — they had each written their own and
  drifted into arriving differently — and the dismiss X is `ui/CloseButton.svelte` for the same
  reason. A filled `--danger` surface uses `--danger-on`, not `--surface`: a background token
  standing in for a foreground worked only because the two invert together between themes, which
  hid the one pairing that has to be contrast-checked.
- **`lib/sections/`** — one component per navigation entry, all wrapped in `Section.svelte` (sticky
  translucent header + one measured column). `InputGestures.svelte` is the old 接收器快捷键 tab, renamed
  after what it actually does and now explaining that both gestures feed the pie menu; it hosts the
  mic-tap feedback panel described under `tap_feedback.rs` above, with restore-factory behind a
  confirmation `Dialog`. `QuickMenu.svelte` and `Preferences.svelte` are surfaces for things that
  previously had no in-window presence at all: the pie menu's hotkey and slot list, and autostart /
  close-to-tray / the tray badge legend / the two loopback ports the Claude Code integration binds.
