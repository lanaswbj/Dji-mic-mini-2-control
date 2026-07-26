<script>
  /**
   * Live audio level. Deliberately *not* spring-animated: a spring overshoots,
   * and an overshooting level meter is a meter that lies. Short linear
   * interpolation only, matching how hardware VU meters ballistics read.
   *
   * The track carries a fixed green->amber->red gradient and an overlay hides
   * everything past the current value, so a quiet signal shows only green.
   */
  let { value = 0, label = "电平" } = $props();

  const pct = $derived(Math.max(0, Math.min(100, Math.round(value))));
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
    <div class="mask" style:transform={`translateX(${pct}%)`}></div>
  </div>
  <output class="u-num">{pct}%</output>
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
     layout path. On 概览 this element is re-laid-out four times a second by
     the live poll, which made it the single worst offender against the
     "only transform and opacity" rule. Linear, not spring: a level meter that
     overshoots is a level meter that lies. */
  .mask {
    position: absolute;
    inset: 0;
    background: var(--surface-sunken);
    will-change: transform;
    transition: transform var(--dur-fast) linear;
  }

  output {
    min-width: 4ch;
    text-align: right;
    font-size: var(--type-caption-size);
    color: var(--text-secondary);
  }
</style>
