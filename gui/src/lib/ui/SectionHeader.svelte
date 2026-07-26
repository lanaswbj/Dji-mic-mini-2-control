<script>
  import Icon from "./Icon.svelte";

  /**
   * The sticky header for a content section.
   *
   * Deliberately a translucent material with content scrolling *underneath*,
   * separated by a fade rather than a 1px rule. Apple's scroll-edge rule: a
   * hard divider under floating chrome reads as a seam; a short gradient mask
   * only where the two actually overlap reads as depth — and only *while* they
   * overlap, which is what `--scroll-edge` (published by the scroll container
   * in App.svelte) carries.
   *
   * The glyph is the same one the sidebar shows for this destination, from the
   * same `nav.js` entry, so "where am I" is answered by the same mark in both
   * places rather than by two things that have to be kept in agreement.
   */
  let { title, icon = null, subtitle = null, actions } = $props();
</script>

<header class="section-head">
  {#if icon}
    <span class="glyph"><Icon name={icon} size="md" /></span>
  {/if}
  <div class="titles">
    <h1>{title}</h1>
    {#if subtitle}<p class="u-caption">{subtitle}</p>{/if}
  </div>
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</header>

<style>
  .section-head {
    position: sticky;
    top: 0;
    z-index: 10;
    display: flex;
    align-items: flex-end;
    gap: var(--space-3);
    padding: var(--space-6) var(--space-8) var(--space-4);
    background: var(--material-chrome);
    backdrop-filter: var(--blur-chrome);
  }

  /* The scroll-edge fade: a short gradient hanging off the bottom of the
     header, so content dissolves into the chrome instead of colliding with a
     border. Pointer-transparent so it never eats a click, and absent entirely
     until something is actually scrolled under it. */
  .section-head::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    top: 100%;
    height: var(--space-5);
    pointer-events: none;
    opacity: var(--scroll-edge, 1);
    background: linear-gradient(to bottom, var(--material-content), transparent);
    transition: opacity var(--dur-base) var(--ease-out);
  }

  /* The header aligns to `flex-end` so the actions button lines up with the
     bottom of the title block; the mark must opt out of that, or it would sit
     beside the *subtitle* instead of the title. Pinned to the top and pulled
     up by half the difference between its box and the h1's line box, so its
     centre lands on the centre of the title. */
  .glyph {
    --glyph: 34px;
    display: grid;
    place-items: center;
    width: var(--glyph);
    height: var(--glyph);
    flex: 0 0 auto;
    align-self: flex-start;
    margin-top: calc(
      (var(--type-title-size) * var(--type-title-line) - var(--glyph)) / 2
    );
    border-radius: var(--radius-md);
    background: var(--accent-soft);
    color: var(--accent);
  }

  .titles {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
    flex: 1 1 auto;
  }

  h1 {
    font-size: var(--type-title-size);
    line-height: var(--type-title-line);
    letter-spacing: var(--type-title-track);
    font-weight: 650;
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: 0 0 auto;
  }

  @container content (max-width: 860px) {
    .section-head {
      padding-inline: var(--space-5);
    }
  }
</style>
