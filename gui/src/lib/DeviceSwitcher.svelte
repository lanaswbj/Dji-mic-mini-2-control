<script>
  import Icon from "./ui/Icon.svelte";
  import Popover from "./ui/Popover.svelte";
  import StatusDot from "./ui/StatusDot.svelte";
  import BatteryIcon from "./BatteryIcon.svelte";
  import DevicePicto from "./DevicePicto.svelte";

  /**
   * Which receiver the 设备 sections are talking about. It lives in the title
   * bar, the only chrome the window has left — a scope selector belongs in
   * chrome rather than in the navigation. It sat at the top of the sidebar
   * before that, and before *that* the sidebar itself was the picker: a
   * full-height device list permanently occupying 260px to show, almost always,
   * exactly one device.
   *
   * With one device it collapses to a plain label with no menu at all, since
   * a picker that can only pick one thing is furniture, not a control.
   *
   * `compact` is the title-bar fit: the same control on one line instead of
   * two. The serial is what goes, because it is the half of the identity you
   * only need when there is more than one device — and when there is, the menu
   * still shows it against every entry.
   */
  let { devices = [], selected = null, onselect, compact = false } = $props();

  let open = $state(false);

  const current = $derived(devices.find((d) => d.id === selected) ?? null);
  const single = $derived(devices.length <= 1);
</script>

{#snippet identity(device, inline = false)}
  <span class="picto">
    <DevicePicto pictogram={`${device.pictogram_key}-rx`} size={inline ? 20 : 28} />
  </span>
  <span class="text">
    <span class="name">{device.model_name}</span>
    {#if !inline}
      <span class="u-caption serial">{device.rx_serial ?? device.id}</span>
    {/if}
  </span>
{/snippet}

{#if !current}
  <div class="slot" class:compact>
    <span class="picto"><Icon name="plug" size={compact ? "sm" : "md"} /></span>
    <span class="text"><span class="name">未连接设备</span></span>
  </div>
{:else if single}
  <div class="slot" class:compact>
    {@render identity(current, compact)}
    <StatusDot
      tone={current.connected ? "ok" : "warn"}
      text={current.connected ? "已连接" : "连接中"}
      compact
    />
  </div>
{:else}
  <Popover
    {open}
    align="stretch"
    label="选择设备"
    onopen={() => (open = true)}
    onclose={() => (open = false)}
  >
    {#snippet trigger(toggle, isOpen)}
      <button
        class="slot trigger"
        class:compact
        onclick={toggle}
        aria-expanded={isOpen}
        aria-haspopup="dialog"
      >
        {@render identity(current, compact)}
        <!-- Turns rather than swaps to `chevron-up`: the same arrow rotating
             says "this is the same control, now open", where two glyphs
             trading places is a state you have to read instead of see. -->
        <span class="caret" class:open={isOpen}><Icon name="chevron-down" size="sm" /></span>
      </button>
    {/snippet}
    {#snippet children()}
      <!-- A plain list of buttons, deliberately *not* `role="menu"` +
           `role="menuitemradio"`. Those roles promise a menu's keyboard model —
           arrow keys, Home/End, type-ahead, focus moved into the menu on open —
           and none of it was implemented, so a screen-reader user was told
           "menu" and then found the arrow keys did nothing. (The `<li>`s were
           also missing `role="none"`, which made the item count wrong, and the
           trigger said `aria-haspopup="menu"` while the panel reported
           `role="dialog"`.) Buttons in a labelled panel are reachable by Tab and
           describe themselves accurately, which is worth more than a widget name
           the code does not honour. `aria-current` carries the selection. -->
      <ul class="list">
        {#each devices as d (d.id)}
          <li>
            <button
              class="item"
              class:active={d.id === selected}
              aria-current={d.id === selected ? "true" : undefined}
              onclick={() => {
                onselect?.(d.id);
                open = false;
              }}
            >
              {@render identity(d)}
              <span class="meta">
                {#each d.tx ?? [] as tx, i}
                  {#if tx}
                    <span class="batt"
                      ><span class="u-label">TX{i + 1}</span>
                      <BatteryIcon value={tx.battery} charging={!!tx.charging} size={16} /></span
                    >
                  {/if}
                {/each}
                {#if d.id === selected}<Icon name="check" size="sm" />{/if}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    {/snippet}
  </Popover>
{/if}

<style>
  .slot {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    min-height: 48px;
    padding: var(--space-2) var(--space-3);
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    background: none;
    text-align: left;
  }

  /* The title-bar fit. Only geometry changes — the identity, the status dot and
     the menu are the same control, so nothing here restates them. `.picto`
     keeps a fixed box at both sizes so the text column starts at the same place
     whether a receiver is connected or not. */
  .slot.compact {
    width: auto;
    min-height: 30px;
    gap: var(--space-2);
    padding: var(--space-05) var(--space-2);
    border-radius: var(--radius-sm);
  }
  .slot.compact .picto {
    width: 20px;
    height: 20px;
  }
  .slot.compact .name {
    font-weight: 500;
  }

  .trigger {
    border-color: var(--border);
    background: var(--surface);
    color: var(--text);
    transition: border-color var(--dur-fast) var(--ease-out),
      transform var(--dur-press) var(--ease-out);
  }
  .trigger:hover {
    border-color: var(--border-strong);
  }
  .trigger:active {
    transform: scale(var(--press-scale-wide));
  }

  /* In the title bar the trigger sits on --surface already, so a bordered
     --surface box would be a rectangle drawn around nothing. It surfaces on
     hover instead, the same way the caption buttons next to it do. */
  .trigger.compact,
  .trigger.compact:hover {
    border-color: transparent;
  }
  .trigger.compact {
    background: none;
  }
  .trigger.compact:hover {
    background: var(--surface-sunken);
  }

  .caret {
    display: inline-flex;
    flex: 0 0 auto;
    color: var(--text-tertiary);
    transition: transform var(--dur-base) var(--ease-out),
      color var(--dur-fast) var(--ease-out);
  }
  .caret.open {
    transform: rotate(180deg);
    color: var(--text-secondary);
  }

  .picto {
    display: grid;
    place-items: center;
    width: var(--glyph-md);
    height: var(--glyph-md);
    flex: 0 0 auto;
    color: var(--text-tertiary);
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1 1 auto;
  }

  .name {
    font-size: var(--type-caption-size);
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .serial {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-2);
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text);
    text-align: left;
    transition: background var(--dur-fast) var(--ease-out);
  }
  .item:hover {
    background: var(--surface-sunken);
  }
  .item.active {
    background: var(--accent-soft);
  }

  .meta {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex: 0 0 auto;
    color: var(--accent);
  }

  .batt {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--text-tertiary);
  }
</style>
