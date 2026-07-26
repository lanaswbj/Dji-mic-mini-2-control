<script>
  import Icon from "./Icon.svelte";

  /**
   * A content card. Renders a real `h2` when given a title so the heading
   * outline of every screen is h1 (section) -> h2 (card) -> h3 (sub-block),
   * with no skipped levels.
   *
   * `icon` is decorative and always optional: it names the card's subject at a
   * glance, and takes its color from `tone`, so a warning card's mark is the
   * warning color without the surface itself being tinted.
   */
  let {
    title = null,
    subtitle = null,
    icon = null,
    tone = "default",
    actions,
    children,
  } = $props();
</script>

<section class="card" data-tone={tone}>
  {#if title || actions}
    <header class="head">
      {#if icon}
        <span class="glyph"><Icon name={icon} size="sm" /></span>
      {/if}
      <div class="titles">
        {#if title}<h2>{title}</h2>{/if}
        {#if subtitle}<p class="u-caption">{subtitle}</p>{/if}
      </div>
      {#if actions}<div class="actions">{@render actions()}</div>{/if}
    </header>
  {/if}
  <div class="body">{@render children()}</div>
</section>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    /* Opaque on purpose, even though the window behind it is glass: the
       content plane under this card is already a translucent material, and
       stacking a second one on it is the exact combination Apple's material
       rule forbids. */
    background: var(--surface);
    box-shadow: var(--elev-1), var(--edge-highlight);
  }

  .head {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
  }

  .glyph {
    --glyph: 28px;
    display: grid;
    place-items: center;
    width: var(--glyph);
    height: var(--glyph);
    flex: 0 0 auto;
    /* Centred on the h2's first line rather than flush with the top of the
       header block, which would float it above a two-line title. Derived:
       half the difference between the glyph box and that line box. */
    margin-top: calc(
      (var(--type-title-sm-size) * var(--type-title-sm-line) - var(--glyph)) / 2
    );
    border-radius: var(--radius-sm);
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

  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: 0 0 auto;
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }

  /* A card can carry an advisory tone (e.g. a driver problem) via a tinted
     left edge and a matching mark — never by tinting the whole surface, which
     hurts text contrast in both themes. */
  [data-tone="warn"] {
    border-color: color-mix(in srgb, var(--warn) 45%, var(--border));
    background: linear-gradient(to right, var(--warn-soft), transparent 320px),
      var(--surface);
  }
  [data-tone="warn"] .glyph {
    background: var(--warn-soft);
    color: var(--warn);
  }
  [data-tone="danger"] {
    border-color: color-mix(in srgb, var(--danger) 45%, var(--border));
    background: linear-gradient(to right, var(--danger-soft), transparent 320px),
      var(--surface);
  }
  [data-tone="danger"] .glyph {
    background: var(--danger-soft);
    color: var(--danger);
  }

  /* Below the narrow end of the reading column a long title and its action
     button stop fitting side by side; the action wraps under rather than
     crushing the title into a two-word column. */
  @container content (max-width: 560px) {
    .head {
      flex-wrap: wrap;
    }
    .actions {
      width: 100%;
    }
  }
</style>
