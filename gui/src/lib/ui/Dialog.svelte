<script>
  import Button from "./Button.svelte";
  import Icon from "./Icon.svelte";

  /**
   * Confirmation dialog, built on the native `<dialog>` element so the focus
   * trap, Esc-to-close, background inerting, and focus restoration all come
   * from the platform instead of being re-implemented (and gotten subtly
   * wrong) in JS.
   *
   * Reserved for genuinely destructive, hard-to-reverse actions. Apple's point
   * about forgiveness cuts both ways: a confirmation on everything trains
   * people to click through them, which makes the one that mattered useless.
   */
  let {
    open = false,
    title,
    description = null,
    confirmLabel = "确定",
    cancelLabel = "取消",
    tone = "danger",
    icon = "alert",
    busy = false,
    onconfirm,
    oncancel,
  } = $props();

  let el = $state(null);

  // A `<dialog>` derives no accessible name from its contents, so without these
  // it announces as a bare "dialog" — on the one surface in the app whose whole
  // job is to make sure the user knows what they are about to agree to.
  const id = $props.id();
  const titleId = `${id}-title`;
  const descId = `${id}-desc`;

  $effect(() => {
    if (!el) return;
    if (open && !el.open) el.showModal();
    else if (!open && el.open) el.close();
  });
</script>

<dialog
  bind:this={el}
  class="dialog"
  aria-labelledby={titleId}
  aria-describedby={description ? descId : undefined}
  oncancel={(e) => { e.preventDefault(); oncancel?.(); }}
>
  <div class="head">
    <span class="mark" data-tone={tone}><Icon name={icon} size="md" /></span>
    <div class="titles">
      <h2 id={titleId}>{title}</h2>
      {#if description}<p class="u-caption" id={descId}>{description}</p>{/if}
    </div>
  </div>
  <div class="actions">
    <Button variant="ghost" onclick={() => oncancel?.()}>{cancelLabel}</Button>
    <Button variant={tone === "danger" ? "danger" : "primary"} {busy} onclick={() => onconfirm?.()}>
      {confirmLabel}
    </Button>
  </div>
</dialog>

<style>
  .dialog {
    width: min(440px, calc(100vw - 2 * var(--space-8)));
    padding: var(--space-6);
    border: 1px solid var(--border);
    border-radius: var(--radius-xl);
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--elev-4), var(--edge-highlight);
  }

  /* The scrim and the entrance animation live in app.css, on `dialog` itself.
     They were written identically here and in AccessIssueCard's hand-rolled
     panel, which is how the two modals drifted into arriving differently. */

  .head {
    display: flex;
    gap: var(--space-4);
    margin-bottom: var(--space-6);
  }

  .mark {
    display: grid;
    place-items: center;
    width: var(--glyph-xl);
    height: var(--glyph-xl);
    flex: 0 0 auto;
    border-radius: var(--radius-md);
    background: var(--surface-sunken);
    color: var(--text-tertiary);
  }
  .mark[data-tone="danger"] {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .mark[data-tone="warn"] {
    background: var(--warn-soft);
    color: var(--warn);
  }

  .titles {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
