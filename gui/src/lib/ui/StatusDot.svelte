<script>
  /**
   * A status dot is never allowed to be the only carrier of its meaning —
   * color-blind users and anyone glancing at a small window both need the
   * word. `text` is therefore required, not optional; pass `compact` to
   * render it visually hidden (still read by screen readers) when the
   * surrounding layout already states it.
   *
   * `compact` used to break that rule on the one screen it exists for. Hiding
   * the word leaves a *sighted* user with nothing but hue, and its only use is
   * the title bar, where 已连接 (green) and 连接中 (amber) are the whole
   * message. So a compact dot is shape-coded too: a settled state is a solid
   * disc, an unsettled one a hollow ring — legible with no color vision at all,
   * and it happens to read correctly as "not there yet".
   */
  import Icon from "./Icon.svelte";

  let { tone = "neutral", text, compact = false, pulse = false } = $props();

  // Solid = this is the state now; ring = on the way to one. `neutral` is the
  // absence of a state rather than a settled one, so it rings as well.
  const settled = $derived(tone === "ok" || tone === "danger");
</script>

<span class="status" class:open={compact && !settled} data-tone={tone}>
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

  /* The second channel for a dot with its word hidden. `currentColor` picks up
     whichever tone rule matched above, so this needs no per-tone repetition —
     and the ring is thick enough (2 of the dot's 4px radius) to survive at the
     size the title bar renders it. */
  .open .dot {
    background: transparent;
    box-shadow: inset 0 0 0 2px currentColor;
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
