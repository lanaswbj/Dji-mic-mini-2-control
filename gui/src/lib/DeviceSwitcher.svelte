<script>
  import Icon from "./ui/Icon.svelte";
  import Popover from "./ui/Popover.svelte";
  import StatusDot from "./ui/StatusDot.svelte";
  import BatteryIcon from "./BatteryIcon.svelte";
  import DevicePicto from "./DevicePicto.svelte";

  /**
   * Which receiver the 设备 sections are talking about, at the top of the
   * sidebar — the standard place for a scope selector, and the answer to the
   * old sidebar's problem: a full-height device list permanently occupying
   * 260px to show, almost always, exactly one device.
   *
   * With one device it collapses to a plain label with no menu at all, since
   * a picker that can only pick one thing is furniture, not a control.
   */
  let { devices = [], selected = null, onselect } = $props();

  let open = $state(false);

  const current = $derived(devices.find((d) => d.id === selected) ?? null);
  const single = $derived(devices.length <= 1);
</script>

{#snippet identity(device)}
  <span class="picto"><DevicePicto pictogram={`${device.pictogram_key}-rx`} size={28} /></span>
  <span class="text">
    <span class="name">{device.model_name}</span>
    <span class="u-caption serial">{device.rx_serial ?? device.id}</span>
  </span>
{/snippet}

{#if !current}
  <div class="slot empty">
    <span class="picto"><Icon name="plug" size="md" /></span>
    <span class="text"><span class="name">未连接设备</span></span>
  </div>
{:else if single}
  <div class="slot">
    {@render identity(current)}
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
      <button class="slot trigger" onclick={toggle} aria-expanded={isOpen} aria-haspopup="menu">
        {@render identity(current)}
        <!-- Turns rather than swaps to `chevron-up`: the same arrow rotating
             says "this is the same control, now open", where two glyphs
             trading places is a state you have to read instead of see. -->
        <span class="caret" class:open={isOpen}><Icon name="chevron-down" size="sm" /></span>
      </button>
    {/snippet}
    {#snippet children()}
      <ul class="list" role="menu">
        {#each devices as d (d.id)}
          <li>
            <button
              class="item"
              class:active={d.id === selected}
              role="menuitemradio"
              aria-checked={d.id === selected}
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
    transform: scale(0.99);
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
    width: 28px;
    height: 28px;
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
