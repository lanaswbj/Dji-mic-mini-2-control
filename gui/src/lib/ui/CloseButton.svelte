<script>
  import Icon from "./Icon.svelte";

  /**
   * The dismiss X. One implementation, because there were two and they had
   * already drifted: the toast's was a 28px square with a background
   * transition, the udev dialog's a 32px square with none — so the same glyph
   * doing the same job answered differently depending on which surface it sat
   * on, and one of them sat below the hit-target floor the rest of the app
   * holds to (--control-h: Button, Switch, Segmented).
   *
   * `label` is required rather than defaulted. An icon-only button's accessible
   * name is the only thing that says *what* is being closed, and a shared
   * default would have every one of them announce the same bare "关闭".
   */
  let { label, onclick } = $props();
</script>

<button class="close" type="button" aria-label={label} {onclick}>
  <Icon name="x" size="sm" />
</button>

<style>
  .close {
    display: grid;
    place-items: center;
    width: var(--control-h);
    height: var(--control-h);
    flex: 0 0 auto;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-tertiary);
    transition: background var(--dur-fast) var(--ease-out),
      color var(--dur-fast) var(--ease-out),
      transform var(--dur-press) var(--ease-out);
  }
  .close:hover {
    background: var(--surface-sunken);
    color: var(--text);
  }
  .close:active {
    transform: scale(var(--press-scale));
  }
</style>
