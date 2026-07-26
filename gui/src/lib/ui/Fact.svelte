<script>
  import Icon from "./Icon.svelte";

  /**
   * One labelled fact inside a `.u-facts` list — a serial, a firmware string,
   * a port, a sample count.
   *
   * Five components had each written this same `div > dt + dd` by hand, which
   * is how one of them ended up with a `dd` that had quietly lost its tabular
   * numerals. The grid itself still lives in app.css (`.u-facts`, tuned per
   * list through `--fact-min`); this is only the cell.
   *
   * `mono` is on by default because almost every fact here is an identifier
   * whose digits must not jitter as it updates; turn it off for prose values
   * like a model name. `clamp` keeps a long identifier on one line with an
   * ellipsis, for the cards narrow enough that wrapping mid-string looks like
   * a rendering fault rather than a long serial.
   */
  let { label, icon = null, mono = true, clamp = false, children } = $props();
</script>

<div class="fact">
  <dt class="u-caption">
    {#if icon}<Icon name={icon} size="sm" />{/if}
    <span>{label}</span>
  </dt>
  <dd class:u-num={mono} class:clamp>{@render children()}</dd>
</div>

<style>
  dt {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--text-tertiary);
  }

  .clamp {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
