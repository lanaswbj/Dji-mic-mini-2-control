<script>
  /**
   * Live audio level, with real meter ballistics.
   *
   * The device is polled four times a second, and the first version simply
   * handed each sample to a 160ms CSS transition. That is what made it look
   * stuttery: the bar travelled for 160ms and then sat perfectly still for
   * the remaining 90ms of every cycle, so a continuous signal read as four
   * discrete lurches per second. Slowing the transition to match the poll
   * would only trade the stutter for lag on every rise.
   *
   * Hardware meters solve this with asymmetric ballistics, and so does this:
   * a fast attack, so a transient is on screen almost immediately and the bar
   * never under-reports a peak, and a slow exponential release, so between
   * two samples it is always still moving. Nothing is invented — the bar can
   * only ever sit at or below a value the device actually reported, which is
   * the one property a level meter is not allowed to break.
   *
   * Driven from `requestAnimationFrame` rather than a CSS transition because
   * a transition can only interpolate towards the last value it was handed;
   * ballistics need the current on-screen value as their starting point on
   * every frame — the same reason Apple animates from the presentation value
   * rather than the model value.
   *
   * The track carries a fixed green->amber->red gradient and an overlay hides
   * everything past the current value, so a quiet signal shows only green.
   */
  let { value = 0, label = "电平" } = $props();

  const target = $derived(Math.max(0, Math.min(100, value)));

  /** Time constants in seconds. Attack is near-instant; release is what the
   *  eye reads as "the sound is dying away" rather than "the UI is lagging". */
  const ATTACK_TAU = 0.04;
  const RELEASE_TAU = 0.32;
  const MAX_DT = 1 / 30;

  let shown = $state(0);
  let frame = 0;
  let last = 0;

  function step(now) {
    const dt = Math.min((now - last) / 1000, MAX_DT);
    last = now;

    const t = target;
    // Exponential approach, framerate-independent: the same curve whether the
    // display runs at 60Hz or 144Hz.
    const tau = t > shown ? ATTACK_TAU : RELEASE_TAU;
    shown += (t - shown) * (1 - Math.exp(-dt / tau));

    if (Math.abs(t - shown) < 0.05) {
      shown = t;
      frame = 0;
      return; // settled — no loop left running on a silent mic
    }
    frame = requestAnimationFrame(step);
  }

  $effect(() => {
    void target; // a new sample restarts the approach from wherever it is now
    if (!frame) {
      last = performance.now();
      frame = requestAnimationFrame(step);
    }
    return () => {
      cancelAnimationFrame(frame);
      frame = 0;
    };
  });

  const pct = $derived(Math.round(shown));
</script>

<div class="meter-row">
  <span class="u-label">{label}</span>
  <div
    class="track"
    role="meter"
    aria-label={label}
    aria-valuenow={pct}
    aria-valuemin="0"
    aria-valuemax="100"
    aria-valuetext={`${pct}%`}
  >
    <div class="mask" style:transform={`translateX(${shown}%)`}></div>
  </div>
  <!-- A plain span, not `<output>`. `<output>` maps to role="status", which is an
       implicit aria-live="polite" region — and `pct` is recomputed from the rAF
       ballistics on *every frame*, once per transmitter. A screen reader was
       therefore queueing an endless stream of percentages for as long as any
       signal was present. The `role="meter"` above already carries the value
       properly (aria-valuenow/valuetext); this is its visible duplicate and has
       no business announcing anything. -->
  <span class="u-num value">{pct}%</span>
</div>

<style>
  .meter-row {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-3);
  }

  .track {
    position: relative;
    height: 8px;
    overflow: hidden;
    border-radius: var(--radius-full);
    box-shadow: inset 0 0 0 1px var(--border);
    background: linear-gradient(
      90deg,
      var(--ok) 0%,
      var(--ok) 45%,
      var(--warn) 72%,
      var(--danger) 100%
    );
  }

  /* Slid with `transform`, not `left`. A percentage translateX is relative to
     the element's own width, which here is the full track, so `translateX(60%)`
     uncovers exactly the first 60% — the same geometry `left` gave, off the
     layout path. No `transition`: the ballistics above already produce a new
     value every frame, and a transition on top of them would be a second,
     conflicting interpolation. */
  .mask {
    position: absolute;
    inset: 0;
    background: var(--surface-sunken);
    /* No `will-change`. It used to sit here permanently, which pins a composited
       layer for the whole session on every visit to 概览 — and `step()` parks
       itself on a silent mic, so most of that session is spent animating
       nothing. Chromium promotes an actively animating `transform` on its own;
       a standing hint is the anti-pattern the property warns about. */
  }

  /* app.css's reduced-motion block sets `transition-duration` on `*` with
     `transition-property` defaulting to `all`, which *grants* a 160ms transition
     to elements that deliberately have none — this mask being the clearest case.
     A transition here is a second interpolation fighting the ballistics above,
     and it breaks the invariant that the bar may only sit at or below the value
     actually reported. Reduced motion is honoured by the ballistics being
     value-driven rather than decorative; it must not be honoured by adding an
     animation that was not there. */
  @media (prefers-reduced-motion: reduce) {
    .mask {
      transition: none !important;
    }
  }

  .value {
    min-width: 4ch;
    text-align: right;
    font-size: var(--type-caption-size);
    color: var(--text-secondary);
  }
</style>
