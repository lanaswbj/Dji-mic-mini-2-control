/**
 * The scroll container's **only** wheel owner: a constant-velocity (linear)
 * scroller.
 *
 * ## What it does
 *
 * Chromium animates a wheel notch with its own eased curve, which is the
 * "acceleration" this replaces. Here a wheel event only moves `target`; the
 * frame loop walks `scrollTop` toward it at a **fixed px/s**, so displacement
 * over time is a straight line with no ease-in and no ease-out.
 *
 * **`speed` is recomputed only when input arrives, never per frame.** This is
 * the whole point and it is easy to undo by accident: recomputing it every frame
 * from the remaining distance is exponential decay — precisely the easing being
 * removed. Deriving it from the backlog at intake time instead keeps the slope
 * constant for the duration of a gesture while bounding how far behind the
 * viewport can fall (`LINEAR_TIME`), so a fast flick does not crawl.
 *
 * A precision touchpad's coasting is **not** removed and cannot be: Windows' own
 * driver keeps sending wheel events for up to a second after the fingers leave
 * the pad. Those are real input and are applied 1:1. What is gone is the
 * browser's animation on top of them.
 *
 * ## There is no rubber band, and that is deliberate
 *
 * This module used to end each range with an Apple-style elastic bounce — a
 * critically damped spring, progressive resistance on intake, the excursion
 * published as `--bounce` for `SectionHeader` to cancel. It is gone by request,
 * and the reason is worth keeping so it does not come back on a hunch:
 *
 * **A rubber band tuned for a touchpad is violent under a mouse wheel.** The
 * band's job is to follow a *continuous* gesture, where each event carries a few
 * pixels. A wheel notch is ~100px arriving as one discrete event, so the first
 * event at an end stop threw the page tens of pixels in a single frame and
 * sprang it back. There is no follow-ratio that is both a soft give for a
 * touchpad and not a lurch for a wheel: the two devices differ by more than an
 * order of magnitude in per-event delta.
 *
 * Deleting it also removed a whole class of failure that three separate fixes
 * had already been written for — an excursion stranded in the middle of the
 * range (every wheel event captured, page apparently frozen, something bouncing)
 * and the `--bounce` counter-transform on the sticky section header, which slid
 * the screen's own title out of view whenever the two got out of step. None of
 * that can happen to a scroller that only ever clamps.
 *
 * Reaching an end is now simply that: the target clamps and the content stops.
 *
 * ## Things that are the CSS's job, not this module's
 *
 * **Axis locking.** `overflow-y: auto` computes `overflow-x` to `auto` as well,
 * so every scroll container in this app was horizontally scrollable with nothing
 * to scroll to, and Chromium's elastic overscroll dragged the whole screen
 * sideways. `overflow-x: hidden` plus `overscroll-behavior: none` (App.svelte,
 * app.css) kills that at the source. An older version *also* latched an axis per
 * gesture here and `preventDefault`ed every event that disagreed — which
 * swallowed perfectly good vertical deltas whenever a flick started with a few
 * px of sideways drift. Cross-axis deltas are now simply not read.
 */

/**
 * How long the viewport is allowed to lag its target, in seconds. `speed` is
 * derived from this and the backlog at intake time, so the constant bounds
 * latency rather than setting a pace: a single 100px notch and a 900px flick
 * both finish in about this long, at very different (but each internally
 * constant) speeds.
 */
const LINEAR_TIME = 0.12;
/**
 * Floor on that speed, px/s. Without it a 3px trackpad nudge would be spread
 * over the full LINEAR_TIME and read as mush; with it, small deltas land
 * essentially immediately.
 */
const MIN_SPEED = 1200;
/** A frame longer than this is a stall; integrating it whole would jump. */
const MAX_DT = 1 / 30;
/**
 * Slack for comparing scroll offsets. Fractional scroll positions and device
 * pixel ratios mean `scrollTop` rarely lands on an exact integer, and the
 * browser rounds what we write to it.
 */
const EPSILON = 1;

/** Chromium reports line/page deltas too; normalise everything to pixels. */
function pixels(e, node) {
  if (e.deltaMode === 1) return [e.deltaX * 16, e.deltaY * 16];
  if (e.deltaMode === 2) return [e.deltaX * node.clientWidth, e.deltaY * node.clientHeight];
  return [e.deltaX, e.deltaY];
}

/**
 * @param {HTMLElement} node
 * @param {{ axis?: "x" | "y" }} [options]
 */
export function fluidScroll(node, options = {}) {
  const vertical = options.axis !== "x";
  const reduced = globalThis.matchMedia?.("(prefers-reduced-motion: reduce)");

  /** Where the viewport is heading. The one value a wheel event writes. */
  let target = 0;
  /** px/s. Set on intake, held until the next intake. */
  let speed = MIN_SPEED;
  /** The last offset *we* wrote, so an outside scroll can be told apart. */
  let written = 0;
  let frame = 0;
  let lastFrameAt = 0;

  const extent = () =>
    vertical ? node.scrollHeight - node.clientHeight : node.scrollWidth - node.clientWidth;
  const offset = () => (vertical ? node.scrollTop : node.scrollLeft);
  const setOffset = (px) => {
    if (vertical) node.scrollTop = px;
    else node.scrollLeft = px;
    // Read back rather than trusting the write: the browser clamps and rounds.
    written = offset();
  };

  /**
   * Adopt a scroll position this module did not set.
   *
   * `go()` in App.svelte runs `scrollTo({top: 0})` on every navigation, and the
   * arrow keys / PageDown scroll natively (they produce no wheel event, so they
   * are deliberately left alone). Without this the stale `target` would drag the
   * view straight back to wherever it was pointing.
   */
  function adopt() {
    if (Math.abs(offset() - written) > EPSILON) {
      target = offset();
      written = target;
    }
  }

  function run() {
    if (frame) return;
    lastFrameAt = performance.now();
    frame = requestAnimationFrame(tick);
  }

  function tick(now) {
    const dt = Math.min((now - lastFrameAt) / 1000, MAX_DT);
    lastFrameAt = now;

    adopt();

    // Re-clamped every frame, not only on input: the content height moves under
    // us — 概览's live meters change it four times a second — so a target that
    // was valid when it was set can be past the end by now.
    target = Math.max(0, Math.min(extent(), target));

    const gap = target - offset();
    if (Math.abs(gap) > 0.5) {
      const stride = speed * dt;
      if (Math.abs(gap) <= stride) {
        setOffset(target);
        // Absorb the browser's own rounding, or `gap` never reaches zero and
        // the loop spins forever.
        target = offset();
      } else {
        setOffset(offset() + Math.sign(gap) * stride);
      }
    }

    if (Math.abs(target - offset()) <= 0.5) {
      frame = 0;
      return;
    }
    frame = requestAnimationFrame(tick);
  }

  function onWheel(e) {
    // Only this container's own axis is ever read. The other one cannot move it
    // (overflow is hidden that way) and must not be able to cancel it.
    const delta = pixels(e, node)[vertical ? 1 : 0];
    if (delta === 0) return;

    // This module owns the axis, so every event carrying one is ours. Taking
    // only some of them would leave the rest to Chromium's eased animation,
    // which is the acceleration being removed.
    e.preventDefault();

    adopt();

    // Clamped, full stop. Past an end there is nowhere to go and nothing to
    // absorb the excess — see the note on the removed rubber band above.
    target = Math.max(0, Math.min(extent(), target + delta));

    if (reduced?.matches) {
      setOffset(target);
      return;
    }

    // Fixed here, from the backlog as it stands right now, and then held until
    // the next event. Recomputing per frame would make the approach exponential
    // — the easing this exists to remove.
    speed = Math.max(Math.abs(target - offset()) / LINEAR_TIME, MIN_SPEED);

    run();
  }

  node.addEventListener("wheel", onWheel, { passive: false });

  return {
    destroy() {
      node.removeEventListener("wheel", onWheel);
      cancelAnimationFrame(frame);
      frame = 0;
    },
  };
}
