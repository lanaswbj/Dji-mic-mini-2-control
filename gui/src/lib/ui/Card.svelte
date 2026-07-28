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
    /* The responsive container for everything inside a card — `Row` especially.
       Rows used to size themselves against `@container content`, i.e. the whole
       content plane, and that measures the wrong box in two ways at once:

       1. It could never fire. The window's minimum width is 760px, which leaves
          `.content` around 734px, so a `max-width: 560px` query on it needs a
          ~590px window — narrower than the app can be made. It was dead code.
          (The rule began as a viewport media query and was moved to a container
          query to fix exactly this. Deleting the sidebar then made `.content`
          almost the full window width and reintroduced the same failure one
          level up.)
       2. Even at a wide window it is the wrong number. 概览 lays transmitter
          cards out in a two-column grid, so a card there is ~420px however wide
          the window is — its rows were cramped at every size.

       A card is the box a row is actually laid out in, so a card is what a row
       must ask. */
    container: card / inline-size;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-xl);
    /* Opaque — the card is a solid object floating *on* the glass, not a pane
       of it. The plane below (--glass-content) is the app's only translucent
       layer.

       Both other arrangements have shipped and both were wrong, in ways worth
       distinguishing so neither gets "restored":

       - Opaque cards over a *thick* plane (0.48): cards cover nearly the whole
         content area, so the backdrop only showed in the gaps between them —
         technically translucent, visually nothing.
       - Translucent cards (0.62) over that same plane: every line of body text
         then sat on the sum of two alphas (~0.80 effective), which reads as a
         washed-out panel rather than a material, and it inverted the platform
         convention by making the content see-through and the chrome solid.

       What works is opaque cards over a *thin* plane: the plane got much more
       transparent once nothing had to be legible on it, so the material is
       continuous across the whole content area instead of surviving only in
       card gaps, and text contrast here is an exact number again.

       No --glass-gloss: a gloss is what keeps a *translucent* rectangle from
       reading as a flat panel. On an opaque surface it is just a white streak.
       --glass-sheen stays — light caught on the top edge is legitimate
       elevation either way, and it is already flattened to nothing when the
       glass is off. */
    background: var(--material-card);
    box-shadow: var(--glass-sheen), var(--elev-2);
  }

  .head {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
  }

  .glyph {
    --glyph: var(--glyph-md);
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
      var(--material-card);
  }
  [data-tone="warn"] .glyph {
    background: var(--warn-soft);
    color: var(--warn);
  }
  [data-tone="danger"] {
    border-color: color-mix(in srgb, var(--danger) 45%, var(--border));
    background: linear-gradient(to right, var(--danger-soft), transparent 320px),
      var(--material-card);
  }
  [data-tone="danger"] .glyph {
    background: var(--danger-soft);
    color: var(--danger);
  }

  /* Below the narrow end of the reading column a long title and its action
     button stop fitting side by side; the action wraps under rather than
     crushing the title into a two-word column. */
  @container card (max-width: 460px) {
    .head {
      flex-wrap: wrap;
    }
    .actions {
      width: 100%;
    }
  }
</style>
