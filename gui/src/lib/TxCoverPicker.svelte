<script>
  import sprite from "../assets/devices/mic-mini-2-cover-cutouts.png";
  import { txCovers, txCover, txCoverPosition } from "./txCovers.js";
  import Icon from "./ui/Icon.svelte";
  import Popover from "./ui/Popover.svelte";

  /**
   * Which magnetic front cover this transmitter is wearing — cosmetic only
   * (see covers.svelte.js). Each swatch carries its real name at readable
   * size; the old picker labelled them at 9px, which is unreadable and below
   * the type scale's floor.
   */
  let { value = "obsidian-black", size = 32, onchange } = $props();

  let open = $state(false);
  const selected = $derived(txCover(value));
</script>

<Popover
  {open}
  align="end"
  label="磁吸前盖"
  onopen={() => (open = true)}
  onclose={() => (open = false)}
>
  {#snippet trigger(toggle, isOpen)}
    <button
      class="trigger"
      onclick={toggle}
      aria-expanded={isOpen}
      aria-haspopup="dialog"
      aria-label={`更换磁吸前盖，当前为${selected.name}`}
      title={`磁吸前盖：${selected.name}`}
    >
      <span
        class="chip"
        style:width={`${size}px`}
        style:height={`${size}px`}
        style:background-image={`url(${sprite})`}
        style:background-position={txCoverPosition(selected)}
      ></span>
      <span class="mark" aria-hidden="true"><Icon name="palette" size="sm" /></span>
    </button>
  {/snippet}

  {#snippet children()}
    <p class="u-label title">磁吸前盖</p>
    <!-- Plain buttons, not a `role="menu"` — same reasoning as DeviceSwitcher's
         list: the menu roles promised a keyboard model that was never
         implemented. -->
    <ul class="swatches">
      {#each txCovers as cover (cover.id)}
        <li>
          <button
            class="swatch"
            class:active={cover.id === selected.id}
            aria-current={cover.id === selected.id ? "true" : undefined}
            onclick={() => {
              onchange?.(cover.id);
              open = false;
            }}
          >
            <span class="dot" style:background={cover.swatch}></span>
            <span class="name">{cover.name}</span>
            {#if cover.id === selected.id}<Icon name="check" size="sm" />{/if}
          </button>
        </li>
      {/each}
    </ul>
  {/snippet}
</Popover>

<style>
  .trigger {
    position: relative;
    display: block;
    padding: 0;
    border: none;
    border-radius: var(--radius-md);
    background: none;
    line-height: 0;
    transition: transform var(--dur-press) var(--ease-out);
  }
  .trigger:active {
    transform: scale(var(--press-scale-lg));
  }

  .chip {
    display: block;
    background-repeat: no-repeat;
    background-size: 500% 200%;
  }

  /* The plate is --glyph-sm rather than the 18px it used to be, and that is the
     whole fix: at 18px the icon inside had to be forced down to 10px through a
     `:global(svg)` override, which was the app's *fourth* icon size — the one
     thing Icon.svelte's doc comment says is a design-system violation rather
     than a prop. --glyph-sm less the 2px ring leaves exactly enough room for a
     real --icon-sm glyph, so the override is gone instead of tokenised. */
  .mark {
    position: absolute;
    right: -4px;
    bottom: -4px;
    display: grid;
    place-items: center;
    width: var(--glyph-sm);
    height: var(--glyph-sm);
    border: 2px solid var(--surface);
    border-radius: 50%;
    background: var(--accent);
    color: var(--accent-on);
  }

  .title {
    padding: var(--space-1) var(--space-2) var(--space-2);
  }

  .swatches {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-05);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .swatch {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    min-height: var(--control-h);
    padding: var(--space-1) var(--space-2);
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-secondary);
    font-size: var(--type-caption-size);
    text-align: left;
    transition: background var(--dur-fast) var(--ease-out),
      color var(--dur-fast) var(--ease-out);
  }
  .swatch:hover {
    background: var(--surface-sunken);
    color: var(--text);
  }
  .swatch.active {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
  }

  /* The ring and its top highlight are what keep a swatch legible whatever
     color it carries — including the white one — so they relate to the
     swatch, not to the theme (see app.css). */
  .dot {
    width: 18px;
    height: 18px;
    flex: 0 0 auto;
    border-radius: 50%;
    box-shadow: var(--swatch-ring-gloss);
  }

  .name {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
