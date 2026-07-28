<script>
  import Icon from "./Icon.svelte";

  /**
   * Every empty state answers three things: what is missing, why, and the one
   * thing to do about it. An empty state without an action is a dead end.
   */
  let { icon = "info", title, description = null, action } = $props();
</script>

<div class="empty">
  <span class="mark"><Icon name={icon} size="lg" /></span>
  <!-- `h2`, not `h3`. Every empty state in the app sits inside a *titleless*
       Card, so there is no card-level h2 above it — an h3 here jumped the
       outline straight from the section's h1. Visually identical: app.css
       styles h2 and h3 with the same step. -->
  <h2>{title}</h2>
  {#if description}<p class="u-caption">{description}</p>{/if}
  {#if action}<div class="action">{@render action()}</div>{/if}
</div>

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-10) var(--space-6);
    text-align: center;
  }

  .mark {
    display: grid;
    place-items: center;
    width: var(--glyph-xl);
    height: var(--glyph-xl);
    border-radius: var(--radius-lg);
    background: var(--surface-sunken);
    color: var(--text-tertiary);
  }

  p {
    max-width: 46ch;
  }

  .action {
    margin-top: var(--space-2);
  }
</style>
