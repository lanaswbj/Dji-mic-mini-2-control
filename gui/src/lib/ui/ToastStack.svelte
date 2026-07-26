<script>
  import { fly } from "svelte/transition";
  import Icon from "./Icon.svelte";
  import { toasts, dismiss } from "./toasts.svelte.js";

  const ICON = { ok: "check", info: "info", warn: "alert", danger: "alert" };
</script>

<!-- Fixed and pointer-transparent except on the toasts themselves, so it can
     never block the UI underneath. -->
<div class="stack">
  {#each toasts as t (t.id)}
    <div
      class="toast"
      data-tone={t.tone}
      role={t.tone === "danger" ? "alert" : "status"}
      in:fly={{ y: 12, duration: 240 }}
      out:fly={{ y: 8, duration: 160 }}
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
      <button class="close" onclick={() => dismiss(t.id)} aria-label="关闭提示">
        <Icon name="x" size="sm" />
      </button>
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
    width: 22px;
    height: 22px;
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

  .act,
  .close {
    flex: 0 0 auto;
    border: none;
    background: none;
    border-radius: var(--radius-sm);
    transition: background var(--dur-fast) var(--ease-out);
  }

  .act {
    min-height: 28px;
    padding: 0 var(--space-3);
    color: var(--accent);
    font-size: var(--type-caption-size);
    font-weight: 600;
  }
  .act:hover {
    background: var(--accent-soft);
  }

  .close {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    color: var(--text-tertiary);
  }
  .close:hover {
    background: var(--surface-sunken);
    color: var(--text);
  }
</style>
