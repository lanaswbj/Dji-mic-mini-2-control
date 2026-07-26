<script>
  /**
   * A status dot is never allowed to be the only carrier of its meaning —
   * color-blind users and anyone glancing at a small window both need the
   * word. `text` is therefore required, not optional; pass `compact` to
   * render it visually hidden (still read by screen readers) when the
   * surrounding layout already states it.
   */
  import Icon from "./Icon.svelte";

  let { tone = "neutral", text, compact = false, pulse = false } = $props();
</script>

<span class="status" data-tone={tone}>
  <span class="dot" class:pulse aria-hidden="true"></span>
  {#if tone === "danger"}<Icon name="alert" size="sm" />{/if}
  <span class:u-sr-only={compact}>{text}</span>
</span>

<style>
  .status {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--type-caption-size);
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .dot {
    width: 8px;
    height: 8px;
    flex: 0 0 auto;
    border-radius: 50%;
    background: var(--border-strong);
    transition: background var(--dur-fast) var(--ease-out),
      box-shadow var(--dur-fast) var(--ease-out);
  }

  [data-tone="ok"] {
    color: var(--ok);
  }
  [data-tone="ok"] .dot {
    background: var(--ok);
    box-shadow: 0 0 0 3px var(--ok-soft);
  }

  [data-tone="warn"] {
    color: var(--warn);
  }
  [data-tone="warn"] .dot {
    background: var(--warn);
    box-shadow: 0 0 0 3px var(--warn-soft);
  }

  [data-tone="danger"] {
    color: var(--danger);
  }
  [data-tone="danger"] .dot {
    background: var(--danger);
    box-shadow: 0 0 0 3px var(--danger-soft);
  }

  [data-tone="accent"] {
    color: var(--accent);
  }
  [data-tone="accent"] .dot {
    background: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }

  /* Used for live detection indicators (a tap was just heard). Deliberately
     an opacity pulse rather than a scale — it must not shift layout. */
  .dot.pulse {
    animation: dot-pulse var(--dur-pulse) ease-in-out infinite;
  }
  @keyframes dot-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.45;
    }
  }
</style>
