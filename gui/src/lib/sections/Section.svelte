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
    max-width: 980px;
    padding: var(--space-2) var(--space-8) var(--space-12);
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
