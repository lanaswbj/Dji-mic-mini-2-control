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
  /** Whatever had focus when the panel opened, so it can be handed back. */
  let returnFocusTo = null;

  function toggle() {
    if (open) onclose?.();
    else onopen?.();
  }

  $effect(() => {
    if (!open) return;
    returnFocusTo = document.activeElement;

    const onDown = (e) => {
      if (wrap && !wrap.contains(e.target)) onclose?.();
    };
    const onKey = (e) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        // Also `preventDefault`: a popover inside a modal `<dialog>` would
        // otherwise let the same Escape cancel the dialog underneath it, so one
        // keypress dismissed two layers.
        e.preventDefault();
        onclose?.();
      }
    };
    // The dismissal this component's doc comment has always promised and never
    // implemented. `relatedTarget` is where focus is *going*; null means it left
    // the window entirely, which is not a dismissal.
    const onFocusOut = (e) => {
      if (e.relatedTarget && wrap && !wrap.contains(e.relatedTarget)) onclose?.();
    };
    // `pointerdown` rather than `click`: dismissal should feel as immediate as
    // every other press-driven response in the app.
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    wrap?.addEventListener("focusout", onFocusOut);

    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
      wrap?.removeEventListener("focusout", onFocusOut);

      // `{#if open}` *unmounts* the panel, so the focused item inside it is
      // simply removed and focus falls to <body> — picking a device or a cover
      // colour with the keyboard dropped the user at the top of the document,
      // and the next Tab restarted from the window's first control.
      //
      // Deferred a frame because the unmount and this cleanup share one flush,
      // so `document.activeElement` is not yet settled here. And guarded on
      // focus actually being loose: if the user has already clicked something
      // else, taking it back would be worse than leaving it alone.
      const back = returnFocusTo;
      returnFocusTo = null;
      if (!back?.isConnected) return;
      requestAnimationFrame(() => {
        const active = document.activeElement;
        if (!active || active === document.body) back.focus();
      });
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
    /* Bounded to the window, and scrollable rather than overflowing it.
       This panel is `position: absolute` with no clipping ancestor, and the
       device switcher's copy hangs off the title bar — so a long enough list
       pushed content past the viewport and gave `body` scrollable overflow.
       `body` was `overflow: hidden`, which suppresses the scrollbar but is
       still a scroll container, so any focus-scroll or a wheel event on the
       chrome then scrolled the *document*: `.app` slid up and the opaque title
       bar left a band of bare window behind it. app.css now makes `body`
       `overflow: clip` so that can never happen again, but leaving the panel
       able to run off-screen would still hide the items at the bottom of it.
       Both halves are needed: one closes the failure mode, this one removes
       the cause.
       `contain` rather than `none`: this panel *should* absorb its own
       overscroll, it just must not chain out of it. */
    max-height: calc(
      100vh - var(--titlebar-h) - 2 * var(--panel-gap) - var(--space-4)
    );
    overflow-y: auto;
    overscroll-behavior: contain;
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
