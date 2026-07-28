<script>
  import { fly } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import CloseButton from "./CloseButton.svelte";
  import { toasts, dismiss } from "./toasts.svelte.js";

  const ICON = { ok: "check", info: "info", warn: "alert", danger: "alert" };

  /**
   * app.css's `prefers-reduced-motion` block cannot reach the transitions below.
   * It works by overriding `transition-duration` in CSS, and a Svelte
   * transition is JS driving inline styles frame by frame — so toasts kept
   * flying 12px for a user who had asked for no motion. Read once at module
   * load: this is a display preference, not something that changes mid-session.
   *
   * `MOTION` is a factor rather than a branch so both the travel and the
   * duration collapse together; a 0ms fly that still moves 12px would snap.
   */
  const REDUCED = globalThis.matchMedia?.("(prefers-reduced-motion: reduce)");
  const MOTION = REDUCED?.matches ? 0 : 1;
  /**
   * Hand-synced with `--dur-base` / `--dur-fast` in app.css. A JS transition has
   * no cheap way to read a custom property, so this is the one place in the app
   * where a duration is duplicated instead of referenced — if those tokens
   * change, these change with them.
   */
  const IN_MS = 240;
  const OUT_MS = 160;
</script>

<!-- Fixed and pointer-transparent except on the toasts themselves, so it can
     never block the UI underneath. -->
<div class="stack">
  {#each toasts as t (t.id)}
    <div
      class="toast"
      data-tone={t.tone}
      role={t.tone === "danger" ? "alert" : "status"}
      in:fly={{ y: 12 * MOTION, duration: IN_MS * MOTION }}
      out:fly={{ y: 8 * MOTION, duration: OUT_MS * MOTION }}
    >
      <span class="mark"><Icon name={ICON[t.tone] ?? "info"} size="sm" /></span>
      <div class="body">
        <p class="title">{t.title}</p>
        {#if t.detail}<p class="u-caption detail">{t.detail}</p>{/if}
      </div>
      {#if t.action}
        <button class="act" onclick={() => { t.action.run(); dismiss(t.id); }}>
          {t.action.label}
        </button>
      {/if}
      <CloseButton label={`关闭提示：${t.title}`} onclick={() => dismiss(t.id)} />
    </div>
  {/each}
</div>

<style>
  .stack {
    position: fixed;
    right: var(--space-5);
    bottom: var(--space-5);
    z-index: 200;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    align-items: flex-end;
    pointer-events: none;
  }

  .toast {
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    width: min(420px, calc(100vw - 2 * var(--space-5)));
    padding: var(--space-3) var(--space-3) var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--material-popover);
    backdrop-filter: var(--blur-popover);
    box-shadow: var(--elev-3), var(--edge-highlight);
  }

  .mark {
    display: grid;
    place-items: center;
    width: var(--glyph-sm);
    height: var(--glyph-sm);
    flex: 0 0 auto;
    border-radius: var(--radius-sm);
    color: var(--text-tertiary);
    background: var(--surface-sunken);
  }

  [data-tone="ok"] .mark {
    color: var(--ok);
    background: var(--ok-soft);
  }
  [data-tone="warn"] .mark {
    color: var(--warn);
    background: var(--warn-soft);
  }
  [data-tone="danger"] .mark {
    color: var(--danger);
    background: var(--danger-soft);
  }
  [data-tone="danger"] {
    border-color: color-mix(in srgb, var(--danger) 40%, var(--border));
  }

  .body {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .title {
    font-size: var(--type-caption-size);
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .detail {
    color: var(--text-secondary);
    overflow-wrap: anywhere;
  }

  /* The dismiss X is `ui/CloseButton.svelte` — it was a local 28px copy that
     had drifted from the identical one in AccessIssueCard, and 28px was the
     app's only sub---control-h target. */
  .act {
    flex: 0 0 auto;
    min-height: var(--control-h);
    padding: 0 var(--space-3);
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--accent);
    font-size: var(--type-caption-size);
    font-weight: 600;
    transition: background var(--dur-fast) var(--ease-out),
      transform var(--dur-press) var(--ease-out);
  }
  .act:hover {
    background: var(--accent-soft);
  }
  .act:active {
    transform: scale(var(--press-scale));
  }
</style>
