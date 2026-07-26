<script>
  import Card from "./ui/Card.svelte";
  import Meter from "./ui/Meter.svelte";
  import Fact from "./ui/Fact.svelte";
  import Icon from "./ui/Icon.svelte";
  import SettingRow from "./SettingRow.svelte";
  import BatteryIcon from "./BatteryIcon.svelte";
  import DevicePicto from "./DevicePicto.svelte";
  import TxArtwork from "./TxArtwork.svelte";
  import TxCoverPicker from "./TxCoverPicker.svelte";
  import { covers } from "./covers.svelte.js";
  import { devices as store, ncPower } from "./store.svelte.js";

  /**
   * One transmitter. The old build rendered this two completely different
   * ways — a "flip" layout for a Mic Mini 2 on v2 firmware and a plain
   * key/value list for everything else — with two different level meters for
   * the same signal. There is one layout now; what varies is only which
   * parts have data to show.
   */
  let { tx, index } = $props();

  // Artwork depends on the transmitter's own reported product name. A brief
  // null on v2 firmware means "hasn't arrived yet", not "unknown forever", so
  // it renders nothing rather than guessing the wrong picture and swapping it.
  const PICTOGRAMS = { "DJI Mic Mini 2": "mic-mini-2" };
  const version = $derived(store.status?.protocol_version);
  const key = $derived(store.device?.pictogram_key);
  const picto = $derived.by(() => {
    if (!tx) return null;
    if (tx.product_name) return `${PICTOGRAMS[tx.product_name] ?? key}-tx`;
    return version === 2 ? null : `${key}-tx`;
  });
  const isMini2 = $derived(tx?.product_name === "DJI Mic Mini 2");

  const title = $derived(tx?.product_name ? `${tx.product_name}（发射器 ${index + 1}）` : `发射器 ${index + 1}`);
  // Levels sit roughly in the 0x14–0x4f range; 80 maps that onto a full bar.
  const level = $derived(tx?.level == null ? 0 : (tx.level / 80) * 100);
  const power = $derived(ncPower(tx));

  /** The per-transmitter settings this model addresses individually. */
  const rows = $derived(
    [
      ["noise-cancel-power", power],
      ["noise-cancel", tx?.nc_mode ?? null],
      ["voice-tone", tx?.voice_tone ?? null],
    ]
      .map(([id, value]) => ({ setting: store.settingsById[id], value }))
      .filter((r) => r.setting),
  );

  const unknown = (v) => v ?? "未知";
</script>

<Card>
  <header class="head">
    <div class="art">
      {#if isMini2}
        <TxArtwork value={covers.get(tx, index)} size={72} />
      {:else if picto}
        <DevicePicto pictogram={picto} size={48} />
      {:else}
        <span class="art-empty"><Icon name="mic" size="lg" /></span>
      {/if}
    </div>

    <div class="titles">
      <h2>{title}</h2>
      {#if tx}
        <span class="batt">
          <BatteryIcon value={tx.battery} charging={!!tx.charging} />
          <span class="u-caption">{tx.charging ? "充电中" : "电量"}</span>
        </span>
      {:else}
        <p class="u-caption">未连接</p>
      {/if}
    </div>

    {#if isMini2}
      <TxCoverPicker
        value={covers.get(tx, index)}
        onchange={(color) => covers.set(tx, index, color)}
      />
    {/if}
  </header>

  {#if tx}
    <dl class="u-facts identity">
      <Fact label="序列号" icon="tag" clamp>{unknown(tx.serial)}</Fact>
      <Fact label="固件" icon="chip" clamp>{unknown(tx.firmware)}</Fact>
    </dl>

    <Meter value={level} label="实时电平" />

    {#if rows.length > 0}
      <div class="rows">
        {#each rows as row (row.setting.id)}
          <SettingRow
            setting={row.setting}
            value={row.value}
            state={store.writeState(row.setting.id)}
            lockReason={row.setting.id === "noise-cancel" && power === "off"
              ? "请先开启该发射器的降噪"
              : store.lockReason(row.setting)}
            onchange={(id, value) => store.changeTx(index, id, value)}
          />
        {/each}
      </div>
    {/if}
  {/if}
</Card>

<style>
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }

  .art {
    display: grid;
    place-items: center;
    width: 72px;
    height: 72px;
    flex: 0 0 auto;
  }
  .art-empty {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: var(--radius-md);
    background: var(--surface-sunken);
    color: var(--text-tertiary);
  }

  .titles {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  h2 {
    overflow-wrap: anywhere;
  }

  .batt {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
  }

  /* Rides on .u-facts (app.css), tightened: this pair sits inside a card that
     already has plenty of vertical rhythm around it. A serial has to stay on
     one line here — the card is the narrowest place one is shown — which is
     what Fact's `clamp` is for. */
  .identity {
    --fact-min: 130px;
    row-gap: var(--space-2);
  }

  .rows {
    display: flex;
    flex-direction: column;
    padding-top: var(--space-2);
    border-top: 1px solid var(--border);
  }
</style>
