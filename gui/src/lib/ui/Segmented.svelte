<script>
  /**
   * Segmented picker for enum settings. The selected indicator is a single
   * element that *slides* between segments (transform only) rather than a
   * background color hopping from one button to another — the movement is
   * what tells you the two options are the same control.
   *
   * Arrow keys move between options, matching the radio-group convention.
   */
  let {
    options = [],
    value = null,
    disabled = false,
    label,
    mixed = false,
    onchange,
  } = $props();

  /** Both axes move the selection: the group may be laid out horizontally,
   *  but a keyboard user shouldn't have to know which. */
  const STEP = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 };

  const index = $derived(options.findIndex((o) => o.value === value));

  function choose(v) {
    if (disabled || v === value) return;
    onchange?.(v);
  }

  function onKeydown(e) {
    if (disabled) return;
    const dir = STEP[e.key] ?? 0;
    if (!dir) return;
    e.preventDefault();
    const from = index < 0 ? 0 : index;
    const next = Math.min(options.length - 1, Math.max(0, from + dir));
    if (next === from) return;
    choose(options[next]?.value);
    // Roving tabindex: the selected radio is the one that's tabbable, so
    // focus has to follow the selection or the next Tab leaves the group.
    e.currentTarget.parentElement?.querySelectorAll("button.segment")[next]?.focus();
  }
</script>

<div class="segmented" class:mixed class:disabled role="radiogroup" aria-label={label}>
  {#if index >= 0}
    <span
      class="indicator"
      style:width={`calc((100% - 2 * var(--seg-pad)) / ${options.length})`}
      style:transform={`translateX(${index * 100}%)`}
      aria-hidden="true"
    ></span>
  {/if}
  <!-- Roving tabindex: exactly one segment is ever a tab stop, so the group is a
       single stop and the arrow keys move within it. When nothing is selected
       (`index < 0` — the normal case for a mixed per-transmitter setting) that
       stop is the first segment. It used to be *every* segment, which turned one
       radio group into N separate tab stops. -->
  {#each options as opt, i (opt.value)}
    <button
      type="button"
      class="segment"
      class:active={value === opt.value}
      role="radio"
      aria-checked={value === opt.value}
      tabindex={index < 0 ? (i === 0 ? 0 : -1) : value === opt.value ? 0 : -1}
      {disabled}
      onclick={() => choose(opt.value)}
      onkeydown={onKeydown}
    >
      {opt.label}
    </button>
  {/each}
</div>

<style>
  .segmented {
    position: relative;
    display: inline-grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    /* One source for the track inset. The indicator's offsets and its width
       calc (set inline in the template) both have to agree with this padding, and
       they were three copies of `--space-1` held together by hand. */
    --seg-pad: var(--space-05);
    padding: var(--seg-pad);
    border-radius: var(--radius-full);
    background: var(--surface-sunken);
    box-shadow: inset 0 0 0 1px var(--border);
  }

  .indicator {
    position: absolute;
    top: var(--seg-pad);
    bottom: var(--seg-pad);
    left: var(--seg-pad);
    border-radius: var(--radius-full);
    background: var(--accent);
    box-shadow: var(--elev-1);
    transition: transform var(--dur-spring) var(--ease-spring);
  }

  .segment {
    position: relative;
    z-index: 1;
    /* --control-h, matching Button and Switch — this is the primary enum
       control on every settings screen and it was the one sitting at a 26px hit
       target. The track's own inset shrank to --space-05 to pay for it, so the
       group is 36px overall rather than 40. */
    min-height: var(--control-h);
    padding: var(--space-1) var(--space-4);
    border: none;
    border-radius: var(--radius-full);
    background: none;
    color: var(--text-secondary);
    font-size: var(--type-caption-size);
    font-weight: 500;
    white-space: nowrap;
    transition: color var(--dur-fast) var(--ease-out);
  }

  .segment.active {
    color: var(--accent-on);
    font-weight: 600;
  }

  .segment:not(:disabled):active {
    transform: scale(var(--press-scale));
    transition: transform var(--dur-press) var(--ease-out);
  }

  .mixed {
    box-shadow: inset 0 0 0 1px var(--warn);
  }

  .disabled {
    opacity: var(--disabled-opacity);
  }
  .disabled .segment {
    cursor: default;
  }
</style>
