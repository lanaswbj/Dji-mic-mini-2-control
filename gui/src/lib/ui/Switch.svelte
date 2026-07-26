<script>
  /**
   * The app's only switch. `SettingControl` and `NoiseCancelControl` used to
   * each carry their own copy of this markup + CSS, which had already silently
   * diverged (different knob offsets, a `.small` variant on one side only).
   *
   * `checked` may be `true`, `false`, or `"mixed"` — the last one meaning the
   * two transmitters disagree. Mixed is never communicated by color alone: the
   * caller is expected to render an accompanying explanation (see Row).
   */
  let {
    checked = false,
    disabled = false,
    label,
    describedBy = undefined,
    onchange,
  } = $props();

  const mixed = $derived(checked === "mixed");
  const on = $derived(checked === true);

  function toggle() {
    if (disabled) return;
    // From "mixed", the useful move is to make both agree — turn them on.
    onchange?.(mixed ? true : !on);
  }
</script>

<button
  type="button"
  class="switch"
  class:on
  class:mixed
  role="switch"
  aria-checked={mixed ? "mixed" : on}
  aria-label={label}
  aria-describedby={describedBy}
  {disabled}
  onclick={toggle}
>
  <span class="track">
    <span class="knob"></span>
  </span>
</button>

<style>
  /* The button is the hit target (>=32px tall); the track inside is the
     visual. Padding, not size, is what makes a small control reachable. */
  .switch {
    display: grid;
    place-items: center;
    padding: var(--space-1) var(--space-1);
    border: none;
    background: none;
    border-radius: var(--radius-full);
    min-height: 32px;
  }

  .track {
    position: relative;
    display: block;
    width: 40px;
    height: 24px;
    border-radius: var(--radius-full);
    background: var(--border-strong);
    transition: background var(--dur-fast) var(--ease-out);
  }

  .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    /* Deliberately un-themed (see app.css): the knob is a physical white
       object in both themes, not a surface that follows the page. */
    background: var(--fixed-white);
    box-shadow: var(--knob-shadow);
    /* Spring, not linear: a physical thing settling into place. */
    transition: transform var(--dur-spring) var(--ease-spring);
  }

  .switch.on .track {
    background: var(--accent);
  }
  .switch.on .knob {
    transform: translateX(16px);
  }

  /* Mixed sits the knob dead center — visibly neither state — and uses the
     warning color, but the meaning is carried by adjacent text, never by the
     color alone. */
  .switch.mixed .track {
    background: var(--warn);
  }
  .switch.mixed .knob {
    transform: translateX(8px);
  }

  /* Feedback on press, not on release. */
  .switch:not(:disabled):active .knob {
    transform: scale(0.92) translateX(var(--knob-x, 0));
    transition: transform var(--dur-press) var(--ease-out);
  }
  .switch.on:not(:disabled):active .knob {
    --knob-x: 17px;
  }
  .switch.mixed:not(:disabled):active .knob {
    --knob-x: 8.5px;
  }

  .switch:disabled {
    opacity: 0.45;
  }
</style>
