<script>
  import { onMount, onDestroy } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { pieMenuAnswerQuestion, pieMenuClose, pieMenuSelect } from "./lib/api.js";
  // Icon tables and the fixed-slot list live in lib/pieIcons.js — see that
  // file for why they are not inline here.
  import {
    DEFAULT_ICONS,
    DEFAULT_ITEMS,
    DEFAULT_LABELS,
    QUESTION_ICON_MAP,
    XMARK_ICON,
  } from "./lib/pieIcons.js";
  import { pieOrder } from "./lib/pieOrder.svelte.js";
  import { theme } from "./lib/theme.svelte.js";

  // This window loads the same bundle as the main one but mounts a different
  // root component, so it has to stamp the appearance preference itself —
  // App.svelte's effect never runs here.
  theme.apply();

  // Non-null while showing a Claude Code permission-request question relayed
  // from gui/src-tauri/src/hook_bridge.rs (via the `pie-menu:open` event's
  // `question` field, set in onMount below) instead of the normal 6 fixed
  // slots — `{ icons: string[] }`, 2-4 icon-key strings long. ITEMS/ICONS
  // below derive entirely from this, so every downstream use of
  // ITEMS.length (layout, wraparound, close-index) transparently adapts to
  // however many slots the current mode actually has.
  let pendingQuestion = $state(null);
  // Screen position -> stable slot index (`pie_menu.rs`'s `SLOTS[i].index`,
  // which is what `pie_menu_select` matches on). The user reorders the fixed
  // slots from 快捷菜单 in the main window; this is the only place that
  // knows about it, and `confirmSelection` maps back through it so the backend
  // still receives the number it always did. A question's slots come from the
  // backend already in the order it wants them, so they are never reordered.
  const ORDER = $derived(pendingQuestion ? null : pieOrder.indices);
  const ITEMS = $derived(
    pendingQuestion
      ? pendingQuestion.icons.map((_, i) => `q${i}`)
      : ORDER.map((slot) => DEFAULT_ITEMS[slot]),
  );
  const ICONS = $derived(
    pendingQuestion
      ? pendingQuestion.icons.map((key) => QUESTION_ICON_MAP[key] ?? XMARK_ICON)
      : ORDER.map((slot) => DEFAULT_ICONS[slot]),
  );

  // Fallback geometry for the very first frame, before the backend's
  // `pie-menu:open` payload (actual monitor-derived width/height, see
  // `gui/src-tauri/src/pie_menu.rs`) arrives. Overwritten immediately after.
  const DEFAULT_ARC_WIDTH = 240;

  let arcWidth = $state(DEFAULT_ARC_WIDTH);
  // Extra logical height reserved above the arc for a question's text
  // panel — 0 outside question mode, set from the backend's `panel_height`
  // (see gui/src-tauri/src/pie_menu.rs's `question_panel_height`) in the
  // `pie-menu:open` listener below.
  let panelHeight = $state(0);
  // The actual OS window width — equals `arcWidth` outside question mode,
  // wider than it while a question card is showing (see the backend's
  // `QUESTION_PANEL_WIDTH_FRACTION`), set from `panel_width` in the
  // `pie-menu:open` listener below. `.arc-wrap`/`.question-panel` size to
  // this instead of `arcWidth` directly so the card gets the extra room;
  // the arc SVG itself keeps rendering at `arcWidth` and is re-centered
  // within the now-wider box via `svgOffsetX` below, same for each item
  // button's own position.
  let panelWidth = $state(DEFAULT_ARC_WIDTH);
  const svgOffsetX = $derived((panelWidth - arcWidth) / 2);
  // The bounding box keeps the same 2:1 aspect ratio the Rust side sizes the
  // window to, even though the visible arc (130°, see ARC_SPAN below) no
  // longer reaches all the way down to the box's bottom corners the way a
  // full 180° semicircle would — that empty space below the arc's open ends
  // is intentional, it's what makes the arc read as floating.
  const arcHeight = $derived(arcWidth / 2);
  const R_OUTER = $derived(arcWidth / 2);
  // The arc's pivot point — horizontal and vertical used to coincide
  // (`PIVOT_Y === R_OUTER`) purely because `arcHeight` happened to equal
  // `arcWidth / 2`. Once a question panel can make the box taller than that
  // without changing the arc's own width, they need to be tracked
  // separately: PIVOT_X stays the horizontal center, PIVOT_Y is the
  // *bottom* of the box (arc + panel), so the arc keeps floating at the
  // bottom edge exactly like before panels existed (PIVOT_Y === R_OUTER
  // whenever panelHeight is 0).
  const PIVOT_X = $derived(R_OUTER);
  const PIVOT_Y = $derived(arcHeight + panelHeight);
  // A thick glass "pill" band rather than a thin ring — thick enough that
  // the selection highlight arc (same stroke width) reads as nested inside
  // it rather than a separate shape overlapping it.
  const BAND_FRACTION = 0.44;
  const BAND_THICKNESS = $derived(R_OUTER * BAND_FRACTION);
  const R_INNER = $derived(R_OUTER - BAND_THICKNESS);
  // Both the band and every item sit on this centerline radius.
  const R_MID = $derived((R_OUTER + R_INNER) / 2);
  const ITEM_SIZE = $derived(R_OUTER * 0.3);

  // Only a partial arc (~130°), not a full 180° semicircle — measured from
  // the positive x-axis (0 = right, 90 = straight up, 180 = left).
  const ARC_SPAN = 130;
  const ARC_START = 90 + ARC_SPAN / 2; // 155
  const ARC_END = 90 - ARC_SPAN / 2; // 25
  // Resting angle every item (and the highlight) collapses back to while
  // closed — dead center of the arc, so opening reads as the fan unfolding
  // outward from one point.
  const ANGLE_REST = 90;
  // Angular width of the nested selection-highlight arc segment: kept just
  // barely wider than its own round end-caps, so it reads as close to a
  // plain circle (just a little wider) rather than an elongated segment.
  const HIGHLIGHT_SPAN = $derived(((ARC_START - ARC_END) / (ITEMS.length - 1)) * 0.16);
  // Radial thickness of the highlight block itself, and how far it sits
  // inset from the band's own inner/outer edges — like a small block
  // sliding inside a tube, with just a sliver of clearance on both sides
  // rather than a large gap.
  const HIGHLIGHT_THICKNESS = $derived(BAND_THICKNESS * 0.82);
  // Items are placed on an inset range, not the band's full [ARC_END,
  // ARC_START] extent — that inset exactly equals the highlight's own half
  // width, so a highlight centered on the first/last item never needs to
  // extend past the band's own bounds. (An earlier version instead clamped
  // the highlight's rendered edges directly, which kept it from poking out
  // but shifted its visual center off of the actual selected item at the
  // two ends — this fixes that at the root instead.)
  const ITEM_ARC_START = $derived(ARC_START - HIGHLIGHT_SPAN / 2);
  const ITEM_ARC_END = $derived(ARC_END + HIGHLIGHT_SPAN / 2);
  // How much of the band's own arc length this stroke actually is, in SVG
  // user units — used to "draw" it in with stroke-dasharray/dashoffset
  // rather than fading the whole shape in uniformly (see bandArcLength).
  const BAND_ARC_LENGTH = $derived(R_MID * ((ARC_START - ARC_END) * Math.PI) / 180);
  // Item reveal is staggered left-to-right (see ITEMS.map reveal below)
  // rather than every item fading in from the center simultaneously.
  const REVEAL_STAGGER = 0.12;

  function angleFor(i, n) {
    if (n <= 1) return ANGLE_REST;
    return ITEM_ARC_START + ((ITEM_ARC_END - ITEM_ARC_START) * i) / (n - 1);
  }

  function polar(cx, cy, r, angleDeg) {
    const rad = (angleDeg * Math.PI) / 180;
    return [cx + r * Math.cos(rad), cy - r * Math.sin(rad)];
  }

  // SVG path for an open arc stroke from startAngle to endAngle at radius r,
  // centered on the pivot (cx, cy) — the box's bottom-center, same point the
  // half-circle geometry has always been centered on. `stroke-linecap:round`
  // (set in the markup below) turns each open end into a rounded cap.
  function describeArc(cx, cy, r, startAngle, endAngle) {
    const [sx, sy] = polar(cx, cy, r, startAngle);
    const [ex, ey] = polar(cx, cy, r, endAngle);
    const largeArc = Math.abs(startAngle - endAngle) > 180 ? 1 : 0;
    return `M ${sx} ${sy} A ${r} ${r} 0 ${largeArc} 1 ${ex} ${ey}`;
  }

  // --- Animation -----------------------------------------------------------
  //
  // Reuses the actual animation approach from the referenced oled-ui-astra
  // project (`Core/Src/astra/ui/item/item.h`, `Animation::move()`): a
  // first-order proportional-step ease toward the target each tick, not a
  // physically-simulated spring. Their original is tick-rate-locked
  // (`pos += (target - pos) / (100 - speed)` once per firmware loop
  // iteration); `stepEase` below adapts the same fraction-per-tick to be
  // frame-rate independent by treating it as continuous exponential decay
  // referenced to a 60Hz tick, since a browser rAF loop's dt varies.
  function stepEase(pos, target, speed, dt) {
    if (Math.abs(target - pos) < 0.01) return target;
    const fractionPerTick = 1 / (100 - speed);
    const decay = Math.pow(1 - fractionPerTick, dt * 60);
    return target + (pos - target) * decay;
  }

  const OPEN_SPEED = 90;
  const HIGHLIGHT_SPEED = 94;
  // How the whole assembled menu (band + highlight + items, as one rigid
  // group — see CLOSE_SLIDE_DISTANCE/groupOpacity/groupTranslateY below)
  // slides away on close: straight down and out, fast, uniformly, instead
  // of reversing the same left-to-right draw-in/stagger used for opening.
  const CLOSE_SLIDE_SPEED = 92;
  const CLOSE_SLIDE_DISTANCE = 26;
  const OPEN_EPS = 0.002;
  const ANGLE_EPS = 0.05;

  let openPos = $state(0);
  let openTarget = 0;
  let closeProgress = $state(0);
  let closeTarget = 0;

  // The selection highlight's own angle — items themselves no longer move
  // (they sit permanently at their resting slot, see angleFor + the reveal
  // stagger below), so this is the only angle that still eases, sliding
  // between slots as the highlighted selection changes.
  let highlightAngle = $state(ANGLE_REST);
  let highlightTarget = ANGLE_REST;

  let selected = $state(0);
  /** @type {"closed" | "open" | "closing"} */
  let phase = $state("closed");
  let openedAt = 0;
  let unlisten;

  let rafId = null;
  let lastT = 0;
  let pendingCloseCallback = null;
  let closeFired = false;
  let unlistenMove;

  /**
   * app.css's `prefers-reduced-motion` block cannot reach any of the motion in
   * this file. It works by overriding `transition-duration` in CSS, and every
   * moving thing here — the band's left-to-right draw-in, the staggered item
   * reveal, the sliding highlight, the close slide — is JS writing inline
   * styles frame by frame. So the one surface that appears unannounced, over
   * whatever the user was doing, animated at full travel for someone who had
   * asked for none.
   *
   * Read once at module load: a display preference, not something that changes
   * mid-session.
   */
  const REDUCED = globalThis.matchMedia?.("(prefers-reduced-motion: reduce)");

  function tick(now) {
    // Snap, don't ease. Every derived value (fade, groupOpacity, highlightArcD,
    // itemProgress) stays exactly as correct — it just arrives in one frame,
    // and the close still completes through the same path below.
    if (REDUCED?.matches) {
      openPos = openTarget;
      highlightAngle = highlightTarget;
      closeProgress = closeTarget;
      rafId = null;
      lastT = 0;
      if (phase === "closing" && !closeFired) {
        closeFired = true;
        finishClose();
      }
      return;
    }

    const dt = Math.min((now - lastT) / 1000, 1 / 30);
    lastT = now;

    let moving = false;

    // Frozen while closing — the open/reveal progress no longer reverses;
    // the whole assembled menu instead slides away as one rigid unit via
    // closeProgress below, so the band + items stay at their fully-open
    // appearance throughout.
    if (phase !== "closing") {
      openPos = stepEase(openPos, openTarget, OPEN_SPEED, dt);
      if (Math.abs(openTarget - openPos) > OPEN_EPS) moving = true;
    }

    highlightAngle = stepEase(highlightAngle, highlightTarget, HIGHLIGHT_SPEED, dt);
    if (Math.abs(highlightTarget - highlightAngle) > ANGLE_EPS) moving = true;

    if (phase === "closing") {
      closeProgress = stepEase(closeProgress, closeTarget, CLOSE_SLIDE_SPEED, dt);
      if (Math.abs(closeTarget - closeProgress) > OPEN_EPS) moving = true;
    }

    if (moving) {
      rafId = requestAnimationFrame(tick);
    } else {
      rafId = null;
      lastT = 0;
      if (phase === "closing" && !closeFired) {
        closeFired = true;
        finishClose();
      }
    }
  }

  // Resets everything for the next open, once this close has actually
  // finished (the overlay window itself is about to be hidden by
  // pendingCloseCallback, so this reset is invisible).
  function finishClose() {
    phase = "closed";
    openPos = 0;
    openTarget = 0;
    closeProgress = 0;
    closeTarget = 0;
    highlightAngle = ANGLE_REST;
    highlightTarget = ANGLE_REST;
    pendingCloseCallback?.();
    pendingCloseCallback = null;
    // Reset after the callback fires (which needs to know it was a
    // question being answered) — the next "pie-menu:open" event sets this
    // again regardless, so this is just hygiene, not load-bearing.
    pendingQuestion = null;
  }

  function ensureLoop() {
    if (rafId == null) {
      lastT = 0;
      rafId = requestAnimationFrame(tick);
    }
  }

  function move(delta) {
    // Wraps around at both ends — moving right past the last slot lands on
    // the first one and vice versa, rather than clamping. This matters for
    // mic-tap navigation specifically: the tap classifier routinely
    // misreads a double tap as a single one, so double-tap-moves-left was
    // dropped as unreliable in favor of single-tap-with-wraparound, which
    // reaches every slot using only the more reliable single-tap gesture
    // (see gui/src-tauri/src/mic_tap.rs's finalize_group).
    const next = (selected + delta + ITEMS.length) % ITEMS.length;
    if (next === selected) return;
    selected = next;
    highlightTarget = angleFor(selected, ITEMS.length);
    ensureLoop();
  }

  function closeWithAnim(after) {
    if (phase !== "open") return;
    phase = "closing";
    // The highlight is left exactly where it is — no repositioning — and
    // slides away together with the rest of the menu as one single rigid
    // unit (see groupOpacity/groupTranslateY), not as a separate step.
    pendingCloseCallback = after;
    closeFired = false;
    closeTarget = 1;
    ensureLoop();
    // Safety net: the ease should always cross the epsilon threshold
    // quickly, but guarantee the callback still fires even if a frame stall
    // or tuning change ever left it short of the target indefinitely.
    setTimeout(() => {
      if (!closeFired) {
        closeFired = true;
        finishClose();
      }
    }, 400);
  }

  // Only the last slot ("close"/xmark, matching CLOSE_INDEX in
  // gui/src-tauri/src/pie_menu.rs) actually closes the menu — every other
  // slot fires its action and leaves the menu open, so pressing e.g. "down"
  // several times in a row doesn't require reopening it each time.
  //
  // A pending question (see pendingQuestion above) is a different case:
  // there's no dedicated close slot, because there's nothing to leave open
  // for — a question is answered once (resolving permission_server's held
  // http connection directly, whether it's a Permission or an
  // AskUserQuestion — see that module's doc comment), so every choice
  // closes the overlay, matching the close-slot's own animation/callback
  // shape.
  function confirmSelection() {
    // `selected` is a screen position. The backend only ever speaks in stable
    // slot indices, so anything leaving this function goes through ORDER first.
    // The close branch stays a *positional* test because the close slot is
    // pinned to the last position (lib/pieOrder.svelte.js) and a question
    // overlay — which has no ORDER — reuses the same last-slot convention.
    const index = pendingQuestion ? selected : ORDER[selected];
    if (pendingQuestion) {
      closeWithAnim(() => pieMenuAnswerQuestion(index));
    } else if (selected === ITEMS.length - 1) {
      closeWithAnim(() => pieMenuSelect(index));
    } else {
      // pie_menu_select (Rust) deliberately hands OS focus back to whatever
      // window was active before the menu opened for a moment, so the
      // simulated keystroke lands there instead of on this still-open
      // overlay — but that handoff itself fires a blur event here, which
      // onBlur normally treats as "clicked away, cancel the menu". Suppress
      // that for long enough to cover the round trip (60ms handoff +
      // action + 30ms reclaim, see pie_menu_select) plus margin.
      suppressBlurUntil = Date.now() + 400;
      pieMenuSelect(index);
    }
  }

  function cancel() {
    closeWithAnim(() => pieMenuClose());
  }

  function onKeydown(e) {
    if (phase !== "open") return;
    switch (e.key) {
      case "ArrowLeft":
      case "ArrowUp":
        e.preventDefault();
        move(-1);
        break;
      case "ArrowRight":
      case "ArrowDown":
        e.preventDefault();
        move(1);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        confirmSelection();
        break;
      case "Escape":
        e.preventDefault();
        cancel();
        break;
    }
  }

  // The overlay window is real-focused on open, but the very first focus
  // event right after `show()` can race a spurious blur — ignore blur for a
  // short grace period after opening. Also ignored while suppressBlurUntil
  // is in the future — see confirmSelection's non-close branch.
  //
  // A pending Claude Code question (see pendingQuestion) never auto-cancels
  // on blur at all, unlike the normal 6-slot menu: anything else briefly
  // stealing OS focus (another window popping up, the user alt-tabbing to
  // read more terminal output, ...) is completely routine while a person
  // takes a moment to actually read/decide on a question, and closing the
  // overlay here doesn't cancel the *real* question still waiting in
  // Claude Code's own terminal — it would just silently hide the only UI
  // that shows what it is and lets the pairing button answer it, with no
  // feedback that that happened. The backend's own pending-answer state
  // (`PENDING_ANSWER` in gui/src-tauri/src/pie_menu.rs) has no lifetime tied
  // to this window's focus either, so there's nothing to invalidate by
  // staying open — only Escape (onKeydown) or an actual answer closes it.
  let suppressBlurUntil = 0;
  function onBlur() {
    if (Date.now() < suppressBlurUntil) return;
    if (pendingQuestion) return;
    if (phase === "open" && Date.now() - openedAt > 250) cancel();
  }

  onMount(() => {
    window.addEventListener("keydown", onKeydown);
    window.addEventListener("blur", onBlur);
    listen("pie-menu:open", (event) => {
      if (event.payload?.width) arcWidth = event.payload.width;
      panelWidth = event.payload?.panel_width ?? arcWidth;
      panelHeight = event.payload?.panel_height ?? 0;
      // Set before reading ITEMS.length below — ITEMS/ICONS derive from
      // this, so the very first frame already reflects however many slots
      // this open actually has (6 fixed, or however many icons a pending
      // question sent).
      pendingQuestion = event.payload?.question ?? null;
      // Re-read the user's slot order. This window has its own JS realm, so it
      // never sees the main window's in-memory state — only the localStorage
      // the two share, and only if it asks. Opening is the one moment the
      // order can matter, so it is the only moment worth asking.
      pieOrder.refresh();
      selected = 0;
      openedAt = Date.now();
      phase = "open";
      openTarget = 1;
      // Placed directly at the first slot, not eased there — it should
      // already be sitting on the first item from the very first rendered
      // frame of the reveal, not visibly slide in from center.
      highlightAngle = angleFor(selected, ITEMS.length);
      highlightTarget = angleFor(selected, ITEMS.length);
      ensureLoop();
    }).then((fn) => {
      unlisten = fn;
    });
    // Mirrors the keyboard's Right handling in onKeydown — emitted by
    // gui/src-tauri/src/pie_menu.rs's `navigate` on a single mic tap while
    // this menu is already open. (The pairing button's confirm doesn't go
    // through an event like this — it just simulates a real Enter keypress,
    // which lands on this already-focused window and hits onKeydown's own
    // Enter case directly.)
    listen("pie-menu:move", (event) => {
      if (phase === "open") move(event.payload);
    }).then((fn) => {
      unlistenMove = fn;
    });
  });

  onDestroy(() => {
    window.removeEventListener("keydown", onKeydown);
    window.removeEventListener("blur", onBlur);
    unlisten?.();
    unlistenMove?.();
    if (rafId != null) cancelAnimationFrame(rafId);
  });

  const fade = $derived(Math.max(0, Math.min(1, openPos)));
  // Whole-group close transform: applied once, on the outer wrapper, to the
  // fully-open band + highlight + items together as one rigid unit — not a
  // reverse of the individual left-to-right reveal used for opening.
  const groupOpacity = $derived(phase === "closing" ? 1 - closeProgress : fade);
  const groupTranslateY = $derived(phase === "closing" ? closeProgress * CLOSE_SLIDE_DISTANCE : 0);
  const bandArcD = $derived(describeArc(PIVOT_X, PIVOT_Y, R_MID, ARC_START, ARC_END));
  // The band's path starts (its `M` point) at ARC_START — the left side —
  // and draws toward ARC_END on the right, so shrinking the dashoffset from
  // the full arc length down to 0 reveals it left-to-right, like a stroke
  // being drawn, instead of fading the whole shape in uniformly.
  const bandDashOffset = $derived(BAND_ARC_LENGTH * (1 - fade));
  // Item i's own reveal progress: a window of the overall open progress
  // offset by its index, so items pop in one after another left-to-right
  // instead of all fading in from the center at once. Each item's window
  // still reaches 1 by the time `fade` reaches 1, and the whole reveal
  // reverses symmetrically (right-to-left) on close.
  function itemProgress(i, n) {
    if (n <= 1) return fade;
    const denom = 1 - (n - 1) * REVEAL_STAGGER;
    return Math.max(0, Math.min(1, (fade - i * REVEAL_STAGGER) / denom));
  }
  // Always symmetric around highlightAngle — items sit on the inset
  // [ITEM_ARC_END, ITEM_ARC_START] range specifically so this never needs
  // clamping to stay within the band's own bounds (see ITEM_ARC_START).
  const highlightArcD = $derived(
    describeArc(PIVOT_X, PIVOT_Y, R_MID, highlightAngle + HIGHLIGHT_SPAN / 2, highlightAngle - HIGHLIGHT_SPAN / 2),
  );
</script>

<div class="stage">
  <div
    class="arc-wrap"
    style="width:{panelWidth}px; height:{arcHeight + panelHeight}px; opacity:{groupOpacity}; transform: translateY({groupTranslateY}px);"
  >
    <svg
      class="arc-svg"
      style="left:{svgOffsetX}px; transform: translateY({(1 - fade) * 12}px) scale({0.9 + fade * 0.1});"
      viewBox="0 0 {arcWidth} {arcHeight + panelHeight}"
      width={arcWidth}
      height={arcHeight + panelHeight}
    >
      <!-- Solid pure-white band — "liquid glass" here means the motion
           (the left-to-right draw-in, the sliding highlight), not the
           material/color, so no frosted tint or sheen gradient. Drawn in
           left-to-right via dasharray/dashoffset rather than fading in
           uniformly — see bandDashOffset. -->
      <path
        d={bandArcD}
        fill="none"
        stroke="var(--surface)"
        stroke-width={BAND_THICKNESS}
        stroke-linecap="round"
        stroke-dasharray={BAND_ARC_LENGTH}
        stroke-dashoffset={bandDashOffset}
      />

      {#if phase !== "closed"}
        <!-- Selection highlight: a small arc-shaped block nested at the same
             centerline radius as the band but thinner than the band's own
             thickness, so it reads as a block sliding inside a tube with
             visible clearance on both sides. -->
        <path
          d={highlightArcD}
          fill="none"
          stroke="var(--accent)"
          stroke-width={HIGHLIGHT_THICKNESS}
          stroke-linecap="round"
          opacity={0.85 * fade}
        />
      {/if}
    </svg>

    {#if pendingQuestion}
      {@const isPermission = pendingQuestion.kind === "permission"}
      {@const detail = pendingQuestion.detail ?? ""}
      <!-- Real question/permission text, in the space question_panel_height
           (gui/src-tauri/src/pie_menu.rs) reserves above the arc, rendered as
           a polished "Claude Code is asking you…" card: a quiet brand header,
           a prominent title (the question, or a synthesized "Allow <tool>?"
           for a permission), an optional monospaced detail block (permission
           mode only — the concrete command/target), then one row per option.
           The arc's own tiny icon slots below stay the tactile selector and
           mic-tap target; this card is its readable mirror, with the row whose
           index === `selected` (while open) highlighted in lockstep with the
           arc highlight below. Each row reuses the same per-slot icon and the
           same select-then-confirm path an arc item uses, so a mouse click is
           an optional extra on top of the arc/keys/mic-tap. Plain HTML (not
           SVG <text>) for free text wrapping; a sibling of the SVG so it can
           use normal CSS layout. No independent opacity/close-animation of its
           own — it inherits `groupOpacity` from `.arc-wrap`, fading/sliding
           away with the rest of the menu. The `kind`/`detail` fields are read
           additively: an older backend that omits them degrades to a plain
           question card (kind → "question", no detail block). -->
      <div class="question-panel" style="height:{panelHeight}px;">
        <div class="qc-card">
          <div class="qc-header">
            <span class="qc-star" aria-hidden="true">✳</span>
            <span class="qc-brand">Claude Code</span>
            {#if isPermission}
              <span class="qc-dot" aria-hidden="true">·</span>
              <span class="qc-kind">Permission</span>
            {/if}
          </div>

          <div class="qc-title">
            {#if isPermission}Allow {pendingQuestion.title}?{:else}{pendingQuestion.title}{/if}
          </div>

          {#if isPermission && detail}
            <div class="qc-detail">{detail}</div>
          {/if}

          <div class="qc-options">
            {#each pendingQuestion.labels as label, i (i)}
              {@const icon = ICONS[i]}
              <button
                type="button"
                class="qc-row"
                class:selected={phase === "open" && i === selected}
                aria-current={phase === "open" && i === selected ? "true" : undefined}
                onclick={() => {
                  selected = i;
                  highlightTarget = angleFor(selected, ITEMS.length);
                  confirmSelection();
                }}
              >
                <svg
                  class="qc-row-icon"
                  viewBox={icon.viewBox}
                  width="18"
                  height="18"
                  aria-hidden="true"
                >
                  {#each icon.paths as d (d)}
                    <path {d} fill="currentColor" />
                  {/each}
                </svg>
                <span class="qc-row-label">{label}</span>
              </button>
            {/each}
          </div>
        </div>
      </div>
    {/if}

    <!-- The arc is icon-only, so `selected` is carried entirely by an accent
         block sliding behind one glyph. `aria-current` below is what makes that
         state exist for anything not looking at the pixels. -->
    {#each ITEMS as label, i (label)}
      {@const rad = (angleFor(i, ITEMS.length) * Math.PI) / 180}
      {@const x = PIVOT_X + R_MID * Math.cos(rad) + svgOffsetX}
      {@const y = PIVOT_Y - R_MID * Math.sin(rad)}
      {@const progress = itemProgress(i, ITEMS.length)}
      {@const icon = ICONS[i]}
      {@const name = pendingQuestion
        ? (pendingQuestion.labels?.[i] ?? label)
        : DEFAULT_LABELS[ORDER[i]]}
      <button
        type="button"
        class="item"
        class:selected={phase === "open" && i === selected}
        aria-current={phase === "open" && i === selected ? "true" : undefined}
        aria-label={name}
        title={name}
        style="left:{x}px; top:{y}px; width:{ITEM_SIZE}px; height:{ITEM_SIZE}px; opacity:{progress}; transform: translate(-50%, -50%) scale({0.55 + 0.45 * progress});"
        onclick={() => {
          selected = i;
          highlightTarget = angleFor(selected, ITEMS.length);
          confirmSelection();
        }}
      >
        <svg
          class="item-icon"
          viewBox={icon.viewBox}
          width={ITEM_SIZE * 0.44}
          height={ITEM_SIZE * 0.44}
          aria-hidden="true"
        >
          {#each icon.paths as d (d)}
            <path {d} fill="currentColor" />
          {/each}
        </svg>
      </button>
    {/each}
  </div>
</div>

<style>
  :global(html),
  :global(body),
  :global(#app) {
    background: transparent !important;
  }

  .stage {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: flex-end;
    justify-content: center;
    pointer-events: none;
  }

  .arc-wrap {
    position: relative;
    pointer-events: none;
  }

  .arc-svg {
    position: absolute;
    top: 0;
    /* `left` is set inline (svgOffsetX) — 0 outside question mode, and the
       centering offset needed to keep the arc's own (unchanged) diameter
       centered within a wider `.arc-wrap` while a question card is showing
       (see QUESTION_PANEL_WIDTH_FRACTION in gui/src-tauri/src/pie_menu.rs). */
    pointer-events: none;
    filter: var(--shadow-overlay);
  }

  /* --- Claude Code sync card -------------------------------------------
     Replaces the old flat title + option-pills panel with one polished card
     that reads as "Claude Code is asking you something." It lives in the
     space `question_panel_height` (gui/src-tauri/src/pie_menu.rs) reserves
     above the arc, and is top-anchored inside it: the reserved height is
     budgeted generously (title + one row each, per that file's
     QUESTION_PANEL_* constants), so any slack collects below the card as a
     deliberate floating gap before the arc, exactly like the old panel did.
     The card is intentionally compact and never taller than its reserved
     space — `max-height:100%` + `overflow:hidden` guarantee it can't spill
     down onto the arc even in the rare all-worst-case-lengths question. */
  .question-panel {
    position: absolute;
    top: 0;
    left: 50%;
    transform: translateX(-50%);
    width: 94%;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    justify-content: flex-start;
    pointer-events: none;
    overflow: hidden;
  }

  /* One solid-white "liquid glass" surface (same material/shadow language as
     the arc band), left-aligned like a real dialog rather than the old
     centered pills. */
  .qc-card {
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 11px;
    max-height: 100%;
    overflow: hidden;
    padding: 15px 17px;
    background: var(--surface);
    border-radius: 18px;
    text-align: left;
    filter: var(--shadow-overlay);
  }

  /* Quiet brand line — small and low-contrast, with only the star tinted the
     accent color so it reads as a mark, not a heading. */
  /* The px geometry through this block (gaps, paddings, the card radius) is
     hand-synced with gui/src-tauri/src/pie_menu.rs's QUESTION_PANEL_* height
     constants, which is what sizes the overlay *window*. Retokenising those
     numbers would silently desync the two and leave the card clipped or
     floating in dead space, so they stay literal. The type sizes below are a
     different matter: each already equalled a scale step exactly, so they are
     now named — this was the only place in the app still setting font-size in
     raw pixels. */
  .qc-header {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--type-label-size);
    font-weight: 600;
    line-height: 1;
    letter-spacing: 0.01em;
    color: var(--text-tertiary);
  }
  .qc-star {
    color: var(--accent);
    font-size: var(--type-caption-size);
  }
  .qc-brand {
    color: var(--text-secondary);
  }
  .qc-dot {
    color: var(--border-strong);
  }
  .qc-kind {
    color: var(--text-tertiary);
  }

  /* The one line worth the most weight: the question, or a synthesized
     "Allow <tool>?" for a permission. Up to 3 lines, then ellipsis. Sizing
     tracks gui/src-tauri/src/pie_menu.rs's QUESTION_PANEL_TITLE_HEIGHT —
     kept in sync by hand, see that file's matching comment. */
  .qc-title {
    font-size: var(--type-title-sm-size);
    /* 1.3, not --type-title-sm-line's 1.35: this one *is* load-bearing
       geometry, see the note above .qc-header. */
    line-height: 1.3;
    font-weight: 700;
    color: var(--text);
    display: -webkit-box;
    -webkit-line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  /* Permission mode only: the concrete command/target being requested, shown
     monospaced in a faintly accent-tinted rounded box so it reads as literal
     text to inspect, not prose.

     It **scrolls** past three lines; it used to `-webkit-line-clamp: 3`. On
     every other surface in the app truncation costs a detail. Here it costs the
     end of a command that Enter is about to approve — the ellipsis said "there
     is more" and offered no way whatsoever to see it, while the button that
     runs it stayed one keypress away. The three-line budget is kept because the
     backend sizes the overlay window from it (pie_menu.rs's
     QUESTION_PANEL_TITLE_HEIGHT), so the box occupies exactly the room it did
     before — the rest of the text is now reachable instead of gone. */
  .qc-detail {
    font-family: var(--font-mono);
    font-size: var(--type-caption-size);
    line-height: var(--type-label-line);
    color: var(--text);
    background: color-mix(in srgb, var(--accent) 9%, var(--surface-sunken));
    border-radius: var(--radius-md);
    padding: 9px 12px;
    max-height: calc(3em * var(--type-label-line));
    overflow-y: auto;
    overscroll-behavior: contain;
    overflow-wrap: anywhere;
  }

  /* The options block is deliberately narrower than the title/detail above
     it and centered — reads as a trapezoid (wide top, narrow bottom)
     instead of a plain rectangle, and gives the title/detail the card's
     full width for actual content (the reason they needed more room in the
     first place) while option labels ("Allow", "Deny", ...) rarely need
     that much width anyway. */
  .qc-options {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    width: 86%;
    align-self: center;
  }

  /* One row per option — the readable mirror of an arc slot. Row height
     tracks gui/src-tauri/src/pie_menu.rs's QUESTION_PANEL_ROW_HEIGHT (kept in
     sync by hand). Reset from the <button> defaults; clickable (same
     select-then-confirm path the arc items use) as a mouse nicety, while the
     arc/keys/mic-tap stay the primary selector. */
  .qc-row {
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    width: 100%;
    margin: 0;
    padding: 9px 13px;
    border: none;
    border-radius: 12px;
    background: var(--surface-sunken);
    color: var(--text-secondary);
    font: inherit;
    font-size: var(--type-body-size);
    font-weight: 500;
    text-align: left;
    cursor: pointer;
    pointer-events: auto;
    transition: background var(--dur-fast) var(--ease-out),
      color var(--dur-fast) var(--ease-out),
      transform var(--dur-fast) var(--ease-out),
      box-shadow var(--dur-fast) var(--ease-out);
  }

  /* Mirrors the arc's own accent highlight: the row at `selected` (while
     open) fills with the accent, brightens its text/icon, and lifts slightly
     — the same controller-focus language as the arc's sliding block below. */
  .qc-row.selected {
    background: var(--accent);
    color: var(--accent-on);
    font-weight: 700;
    transform: scale(1.02);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 42%, transparent);
  }

  /* Its paths use fill:currentColor, so it inherits the row's text color and
     flips to white on the selected row exactly like the arc icons do. */
  .qc-row-icon {
    display: block;
    flex: 0 0 auto;
  }

  /* Still clamped, unlike .qc-detail above, and the difference is that an
     option label is bounded by construction — "Allow" / "Deny" / "Allow, don't
     ask again", or an AskUserQuestion's own choice labels. Two lines is a
     backstop, not the normal case, and the row height it would have to grow
     into is one of the numbers pie_menu.rs sizes the window from. */
  .qc-row-label {
    flex: 1 1 auto;
    min-width: 0;
    line-height: var(--type-title-line);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  /* No background/border chrome — just the icon itself sitting directly on
     the band, no separate circular button shape underneath each option.
     Dark by default for contrast against the now-solid-white band; flips to
     white when the accent-colored highlight block is sitting behind it. */
  .item {
    position: absolute;
    display: grid;
    place-items: center;
    background: none;
    border: none;
    color: var(--text);
    cursor: pointer;
    pointer-events: auto;
    transition: color var(--dur-fast) var(--ease-out);
  }

  .item.selected {
    color: var(--accent-on);
  }

  .item-icon {
    pointer-events: none;
  }
</style>
