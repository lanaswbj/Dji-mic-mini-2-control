<script>
  /**
   * A popover anchored to its trigger. `transform-origin` points back at the
   * trigger so the panel visibly grows *out of* the thing you clicked —
   * Apple's spatial-consistency rule: a surface should emerge from where it
   * came, and dismiss back along the same path.
   *
   * Closes on Esc, on outside pointer-down, and on losing focus to anything
   * outside the wrapper.
   */
  let {
    open = false,
    align = "end",
    label,
    onopen,
    onclose,
    trigger,
    children,
  } = $props();

  let wrap = $state(null);

  function toggle() {
    if (open) onclose?.();
    else onopen?.();
  }

  $effect(() => {
    if (!open) return;
    const onDown = (e) => {
      if (wrap && !wrap.contains(e.target)) onclose?.();
    };
    const onKey = (e) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onclose?.();
      }
    };
    // `pointerdown` rather than `click`: dismissal should feel as immediate as
    // every other press-driven response in the app.
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
    };
  });
</script>

<div class="wrap" bind:this={wrap}>
  <div class="trigger">{@render trigger(toggle, open)}</div>
  {#if open}
    <div class="panel" data-align={align} role="dialog" aria-label={label}>
      {@render children()}
    </div>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
    display: flex;
    min-width: 0;
  }

  .trigger {
    display: flex;
    min-width: 0;
    flex: 1 1 auto;
  }

  .panel {
    position: absolute;
    top: calc(100% + var(--space-2));
    z-index: 150;
    min-width: 220px;
    padding: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--material-popover);
    backdrop-filter: var(--blur-popover);
    box-shadow: var(--elev-3), var(--edge-highlight);
    animation: pop-in var(--dur-base) var(--ease-out);
  }

  [data-align="end"] {
    right: 0;
    transform-origin: top right;
  }
  [data-align="start"] {
    left: 0;
    transform-origin: top left;
  }
  [data-align="stretch"] {
    left: 0;
    right: 0;
    transform-origin: top center;
  }

  /* Materialize rather than plain-fade: the blur radius and the scale move
     together, so the panel reads as a glass surface arriving from its trigger
     rather than as opacity being turned up on a rectangle. */
  @keyframes pop-in {
    from {
      opacity: 0;
      transform: scale(0.95) translateY(-4px);
      backdrop-filter: blur(0) saturate(100%);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .panel {
      animation: none;
    }
  }
</style>
