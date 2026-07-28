<script>
  import SectionHeader from "../ui/SectionHeader.svelte";

  /**
   * The frame every section shares: sticky translucent header, then a single
   * measured column of cards. Having exactly one of these is what makes the
   * seven sections feel like one app rather than seven screens that happen to
   * live in the same window.
   */
  let { title, icon = null, subtitle = null, actions, children } = $props();
</script>

<SectionHeader {title} {icon} {subtitle} {actions} />
<div class="stack">{@render children()}</div>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    /* Centred, not left-hugged. Left-aligned this was merely off at the
       default window size and glaring the moment the sidebar collapsed: the
       column stayed pinned left while ~350px of empty plane opened on the
       right. The width is a token because the sticky header has to land on
       the exact same column, and the two lived in different files held
       together by a copied number. */
    width: 100%;
    max-width: var(--measure-wide);
    margin-inline: auto;
    /* The bottom step is the dock's clearance, not a design value: the
       navigation floats *over* this column (App.svelte), so the last card has
       to be able to scroll clear of it or it is permanently half-covered. The
       fallback keeps this component usable in a layout that has no dock. */
    padding: var(--space-2) var(--space-8) var(--dock-clear, var(--space-12));
    /* Content arrives; chrome stays put. The header is deliberately excluded —
       animating the sticky element too would make the whole screen blink on
       every navigation, where moving only what actually changed reads as one
       app changing subject. App.svelte keys this component on the destination
       id so the motion replays even between two sections that happen to share
       a component. */
    animation: section-in var(--dur-base) var(--ease-out) both;
  }

  @keyframes section-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }

  /* Travel is vestibular; the cross-fade that tells you the screen changed is
     not. Keep the second, drop the first. */
  @media (prefers-reduced-motion: reduce) {
    .stack {
      animation: none;
    }
  }

  @container content (max-width: 860px) {
    .stack {
      padding-inline: var(--space-5);
    }
  }
</style>
