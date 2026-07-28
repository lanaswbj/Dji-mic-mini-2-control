<script>
  import Icon from "./ui/Icon.svelte";

  /**
   * The app's navigation, as a floating glass capsule over the content.
   *
   * Replaces a 236px sidebar. The trade the sidebar was making — a permanent
   * quarter of the window spent on seven labels that never change — only ever
   * paid off while the labels were being learned; after that it was a column of
   * text the eye skips on its way to the content. A dock keeps every
   * destination one click away, gives the content the whole window, and is the
   * one piece of chrome that can genuinely float *on* the material rather than
   * next to it.
   *
   * Deliberately **not** reorderable. A first version of this component shipped
   * drag-to-reorder here and it was the wrong control to put it on: every press
   * on a navigation item then has to be disambiguated from the start of a drag,
   * which is a tax paid on every single click to buy a rearrangement nobody
   * performs twice. The pie menu's slots are where reordering earns its place
   * (lib/pieOrder.svelte.js, edited from 快捷菜单 in the main window) — there
   * the order *is* the ergonomics, because it decides which way the selection
   * has to travel to reach the thing you want.
   *
   * Uniform square items, labels in tooltips: the section's own title is on
   * screen the whole time (SectionHeader), so knowing where you are never
   * depends on reading the dock.
   */
  let { items = [], current = null, onnavigate } = $props();

  let railEl = $state(null);

  // --- The selection pill -----------------------------------------------
  // One element that slides between items rather than a background colour
  // hopping from one button to the next. Measured rather than computed from an
  // index, so it stays correct however the dock is sized.
  let pill = $state({ x: 0, w: 0 });
  let pillReady = $state(false);

  $effect(() => {
    void current;
    void items;
    const el = railEl?.querySelector(".item.active");
    if (!el) {
      pill = { x: 0, w: 0 };
      pillReady = false;
      return;
    }
    pill = { x: el.offsetLeft, w: el.offsetWidth };
    // The first placement must not slide in from the left edge; every move
    // after it must. One frame is enough to commit the initial position.
    if (!pillReady) requestAnimationFrame(() => (pillReady = true));
  });
</script>

<nav class="dock" aria-label="主导航">
  <div class="rail" bind:this={railEl}>
    {#if pill.w > 0}
      <span
        class="pill"
        class:ready={pillReady}
        style:width="{pill.w}px"
        style:transform="translateX({pill.x}px)"
        aria-hidden="true"
      ></span>
    {/if}

    {#each items as item (item.id)}
      <button
        class="item"
        class:active={item.id === current}
        aria-current={item.id === current ? "page" : undefined}
        onclick={() => onnavigate?.(item.id)}
      >
        <Icon name={item.icon} size="md" />
        <span class="u-sr-only">{item.label}</span>
        <span class="tip" aria-hidden="true">{item.label}</span>
      </button>
    {/each}
  </div>
</nav>

<style>
  /* Floating, not docked: the capsule sits over the content plane with the
     plane's own material visible all around it, which is the only arrangement
     in which a translucent surface reads as a separate object rather than as a
     lighter region of the same one. */
  .dock {
    position: absolute;
    left: 50%;
    bottom: var(--space-4);
    z-index: 30;
    transform: translateX(-50%);
    /* The item count is data-driven — one entry per `Setting.group` the
       connected model declares (nav.js) — so this component cannot know how
       wide it will be. Without a cap the rail simply grew past the window and
       `.content-clip` cut the ends off, taking whole destinations with them and
       giving no sign it had happened. Capping the dock instead lets the items
       compress (see `.item`), which is visible and reversible.
       Not a horizontal scroller: that would make `.rail` a scroll container,
       and the tooltips hang *above* the rail — they would be clipped away, so
       the fix for one silent truncation would have introduced another. */
    max-width: calc(100vw - 2 * var(--space-4));
  }

  .rail {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1);
    border-radius: var(--radius-full);
    background: var(--material-popover);
    /* The one place in the app where `backdrop-filter` is doing real work: the
       content plane genuinely scrolls underneath this, so there *is* something
       the page painted for it to sample. (On the title bar and the old sidebar
       there was not — see app.css's materials note.) */
    backdrop-filter: var(--blur-popover);
    box-shadow: inset 0 0 0 1px var(--border), var(--glass-sheen), var(--elev-3);
  }

  /* `.item.active` deliberately carries no background of its own — if it did,
     two fills would be lit at once for the length of the slide. */
  .pill {
    position: absolute;
    left: 0;
    top: var(--space-1);
    bottom: var(--space-1);
    border-radius: var(--radius-full);
    background: var(--accent-soft);
    pointer-events: none;
  }
  /* `transform` only. `width` used to be in here too — a 480ms transition on a
     layout property, the only one in the app, running layout on every frame of
     the longest animation there is. And it never did anything: every dock item
     is a uniform --hit square, so the measured `pill.w` is always the same number
     and the property never actually changed. */
  .pill.ready {
    transition: transform var(--dur-spring) var(--ease-spring);
  }

  /* app.css's reduced-motion block sets `transition-duration` on `*`, and
     `transition-property`'s initial value is `all` — so it *grants* a 160ms
     transition to anything that deliberately had none. This pill is one of
     those: it is transition-less until `.ready` precisely so the first placement
     does not slide in from the rail's left edge on every launch. Restated
     locally rather than adding a global opt-out mechanism, which would be a
     bigger thing to remember than three local overrides. */
  @media (prefers-reduced-motion: reduce) {
    .pill:not(.ready) {
      transition: none !important;
    }
  }

  .item {
    position: relative;
    display: grid;
    place-items: center;
    /* --hit, not a literal: App.svelte's --dock-clear (how far the last card
       scrolls clear of this capsule) is computed from the same token, so the
       two can no longer disagree. */
    width: var(--hit);
    height: var(--hit);
    /* Shrinkable, floored at --control-h. Height is deliberately *not* in the
       shrink: --dock-clear is computed from --hit and a capsule that got
       shorter under pressure would leave the last card floating over dead
       space. Only the width gives. */
    flex: 0 1 auto;
    min-width: var(--control-h);
    border: none;
    border-radius: var(--radius-full);
    background: none;
    color: var(--text-secondary);
    transition: color var(--dur-fast) var(--ease-out),
      background var(--dur-fast) var(--ease-out),
      transform var(--dur-press) var(--ease-out);
  }
  .item:hover {
    color: var(--text);
    background: var(--surface-sunken);
  }
  .item:active {
    transform: scale(var(--press-scale-lg));
  }
  .item.active,
  .item.active:hover {
    color: var(--accent);
    background: none;
  }

  /* The label the square costs. Above the dock rather than below it, since the
     dock is already at the bottom of the window. */
  .tip {
    position: absolute;
    bottom: calc(100% + var(--space-2));
    left: 50%;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--surface-raised);
    box-shadow: inset 0 0 0 1px var(--border), var(--elev-2);
    color: var(--text);
    font-size: var(--type-label-size);
    line-height: var(--type-label-line);
    font-weight: 600;
    /* Wraps rather than `nowrap`. Dock labels for the `group:*` entries come
       from the connected model (nav.js), not from this codebase, so a long one
       produced a tooltip wider than the window with nothing to stop it. A
       max-width plus normal wrapping keeps short labels on one line — which is
       every label today — without letting an unknown string run off-screen.
       Not ellipsised: the tooltip exists to reveal the name, so truncating it
       would defeat the control. */
    max-width: min(240px, 40vw);
    text-align: center;
    text-wrap: balance;
    opacity: 0;
    transform: translate(-50%, 4px);
    pointer-events: none;
    transition: opacity var(--dur-fast) var(--ease-out),
      transform var(--dur-fast) var(--ease-out);
  }
  .item:hover .tip,
  .item:focus-visible .tip {
    opacity: 1;
    transform: translate(-50%, 0);
  }
</style>
