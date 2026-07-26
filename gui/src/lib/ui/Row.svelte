<script>
  import Icon from "./Icon.svelte";
  import Spinner from "./Spinner.svelte";

  /**
   * One setting row: an optional subject glyph, label + explanation on the
   * left, control on the right, and — the part the old UI was missing
   * entirely — a write-state slot between them.
   *
   * The old build showed nothing at all between clicking a switch and the
   * device confirming, except `cursor: progress`. There was no way to tell
   * "writing" from "the click didn't register". `state` fixes that:
   *
   *   idle    nothing shown
   *   writing spinner + 写入中
   *   ok      check + 已生效, which the caller clears after ~1.2s
   *   error   alert + the reason, and the row is tinted
   *
   * `lockReason` (why a setting cannot be changed right now) is rendered as
   * real text next to a lock *icon* — the old build used a 🔒 emoji, which is
   * font-dependent, untintable, and unreadable to a screen reader.
   */
  let {
    label,
    icon = null,
    description = null,
    note = null,
    lockReason = null,
    state = "idle",
    control,
  } = $props();

  const id = $props.id();
  const descId = `${id}-desc`;
  const locked = $derived(!!lockReason);
</script>

<div class="row" class:locked data-state={state}>
  {#if icon}
    <span class="glyph"><Icon name={icon} size="sm" /></span>
  {/if}

  <div class="text">
    <span class="label">
      {label}
      {#if locked}
        <span class="lock" title={lockReason}><Icon name="lock" size="sm" label={lockReason} /></span>
      {/if}
    </span>

    {#if description}<p class="u-caption" id={descId}>{description}</p>{/if}
    {#if lockReason}<p class="u-caption reason">{lockReason}</p>{/if}
    {#if note}
      <p class="u-caption u-icon-line flag"><Icon name="info" size="sm" /><span>{note}</span></p>
    {/if}
  </div>

  <div class="right">
    <span class="write-state" aria-live="polite">
      {#if state === "writing"}
        <Spinner size="sm" label="" /><span class="write-text">写入中</span>
      {:else if state === "ok"}
        <Icon name="check" size="sm" /><span class="write-text">已生效</span>
      {:else if state === "error"}
        <Icon name="alert" size="sm" /><span class="write-text">未生效</span>
      {/if}
    </span>
    <div class="control">{@render control(descId)}</div>
  </div>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    min-height: 40px;
  }

  /* Centred on the row, not pinned to the label's first line.
     A first attempt pinned it — the rule that is right for `.u-icon-line`,
     where the glyph leads a *sentence* and has to sit on its first line. It is
     wrong here and it showed: a row glyph marks the row's *subject*, and on the
     single-line rows that make up most of this app it landed ~5px above its own
     label, because the row centres `.text` while a pinned glyph starts at the
     flex line's top. Centred is also what the platform's own settings lists
     do. */
  .glyph {
    --glyph: 28px;
    display: grid;
    place-items: center;
    width: var(--glyph);
    height: var(--glyph);
    flex: 0 0 auto;
    align-self: center;
    border-radius: var(--radius-sm);
    background: var(--surface-sunken);
    color: var(--text-tertiary);
    transition: color var(--dur-fast) var(--ease-out),
      background var(--dur-fast) var(--ease-out);
  }

  /* The glyph joins in on the two states that are worth noticing across the
     room, so neither is carried by one small patch of color alone. */
  [data-state="ok"] .glyph {
    background: var(--ok-soft);
    color: var(--ok);
  }
  [data-state="error"] .glyph {
    background: var(--danger-soft);
    color: var(--danger);
  }

  /* Grows, so a long description wraps into the space that is actually free
     rather than only into its own intrinsic width. */
  .text {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
    flex: 1 1 auto;
  }

  .label {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--type-body-size);
    font-weight: 550;
  }

  .locked .label {
    color: var(--text-secondary);
  }

  .lock {
    display: inline-flex;
    color: var(--text-tertiary);
  }

  .reason {
    color: var(--text-tertiary);
  }

  .flag {
    color: var(--text-secondary);
  }

  .right {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex: 0 0 auto;
    /* The gap between the text and the control is the row's real breathing
       room; the flex gap above only has to keep the glyph off the label. */
    margin-left: var(--space-2);
  }

  /* Fixed-width so a state appearing or clearing never nudges the control
     sideways — feedback must not cause layout shift. */
  .write-state {
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-2);
    min-width: 68px;
    font-size: var(--type-caption-size);
    color: var(--text-tertiary);
    transition: opacity var(--dur-fast) var(--ease-out);
  }

  .write-text {
    white-space: nowrap;
  }

  [data-state="ok"] .write-state {
    color: var(--ok);
  }
  [data-state="error"] .write-state {
    color: var(--danger);
  }

  .control {
    display: flex;
    align-items: center;
  }

  /* Below this the control drops under its label rather than squeezing both —
     a squeezed segmented picker is unreadable.

     Measured against the *content column*, not the viewport. As a viewport
     media query this never fired where it was needed: at the 760px minimum
     window width with the sidebar open the reading column is only ~520px
     wide, so every row stayed side-by-side at exactly the size the rule was
     written to rescue. */
  @container content (max-width: 560px) {
    .row {
      flex-wrap: wrap;
      row-gap: var(--space-2);
    }
    /* Only the control drops to a second line — the glyph stays with the
       label it names, which is the whole reason it is there. */
    .right {
      flex: 1 1 100%;
      justify-content: space-between;
      margin-left: 0;
    }
  }
</style>
