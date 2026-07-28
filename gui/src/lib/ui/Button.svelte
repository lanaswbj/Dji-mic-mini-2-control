<script>
  import Icon from "./Icon.svelte";
  import Spinner from "./Spinner.svelte";

  /**
   * Variants map to intent, not to looks: `primary` is the one obvious action
   * on a screen, `danger` is destructive (and must be paired with a Dialog
   * confirmation), `ghost` is a low-emphasis alternative that still needs a
   * visible hover/press state.
   *
   * `icon` leads the label, `iconEnd` follows it — and `iconEnd` exists as a
   * real prop rather than as "just put an <Icon> in the children" because a
   * trailing glyph belongs to the button's layout, not to its label: it has to
   * be a sibling of the label span so it keeps the same `gap` as the leading
   * icon and never participates in the label's own line wrapping. Passing one
   * inline was what made 概览's「全部音频设置」chevron land on a second row.
   *
   * `spin` turns the leading icon rather than swapping it for a spinner — a
   * refresh that keeps its own glyph while turning reads as "this same action,
   * in progress", where a substituted spinner reads as "something else now".
   */
  let {
    variant = "secondary",
    icon = null,
    iconEnd = null,
    spin = false,
    busy = false,
    disabled = false,
    title = undefined,
    onclick,
    children,
  } = $props();
</script>

<button
  type="button"
  class="btn"
  data-variant={variant}
  disabled={disabled || busy}
  aria-busy={busy}
  {title}
  {onclick}
>
  {#if busy}
    <Spinner size="sm" label="" />
  {:else if icon}
    <span class="glyph" class:spin><Icon name={icon} size="sm" /></span>
  {/if}
  {#if children}<span class="body">{@render children()}</span>{/if}
  {#if iconEnd && !busy}
    <span class="glyph trail"><Icon name={iconEnd} size="sm" /></span>
  {/if}
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    min-height: var(--control-h);
    padding: var(--space-2) var(--space-4);
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    font-size: var(--type-caption-size);
    font-weight: 600;
    white-space: nowrap;
    transition: background var(--dur-fast) var(--ease-out),
      border-color var(--dur-fast) var(--ease-out),
      color var(--dur-fast) var(--ease-out),
      transform var(--dur-press) var(--ease-out);
  }

  /* Press feedback fires on pointer-down and never changes layout bounds. */
  .btn:not(:disabled):active {
    transform: scale(var(--press-scale));
  }

  .btn:disabled {
    opacity: var(--disabled-opacity);
  }

  [data-variant="primary"] {
    background: var(--accent);
    color: var(--accent-on);
  }
  [data-variant="primary"]:not(:disabled):hover {
    background: color-mix(in srgb, var(--accent) 88%, var(--text));
  }

  [data-variant="secondary"] {
    background: var(--surface);
    border-color: var(--border-strong);
    color: var(--text);
  }
  [data-variant="secondary"]:not(:disabled):hover {
    background: var(--surface-raised);
    border-color: var(--text-tertiary);
  }

  [data-variant="ghost"] {
    background: transparent;
    color: var(--text-secondary);
  }
  [data-variant="ghost"]:not(:disabled):hover {
    background: var(--accent-soft);
    color: var(--text);
  }

  [data-variant="danger"] {
    background: var(--danger-soft);
    border-color: color-mix(in srgb, var(--danger) 40%, transparent);
    color: var(--danger);
  }
  /* --danger-on, not --surface. A background token standing in for a foreground
     only ever worked because the two invert together between the themes, which
     hid the one pairing here that has to be contrast-checked. */
  [data-variant="danger"]:not(:disabled):hover {
    background: var(--danger);
    border-color: var(--danger);
    color: var(--danger-on);
  }

  .body {
    min-width: 0;
  }

  .glyph {
    display: inline-flex;
    /* The trailing chevron leans toward its destination on hover — a small
       hint in the direction the gesture is going, not decoration. */
    transition: transform var(--dur-fast) var(--ease-out);
  }
  .btn:not(:disabled):hover .trail {
    transform: translateX(2px);
  }

  .glyph.spin {
    animation: glyph-spin var(--dur-spin) linear infinite;
  }
  @keyframes glyph-spin {
    to {
      transform: rotate(1turn);
    }
  }
</style>
