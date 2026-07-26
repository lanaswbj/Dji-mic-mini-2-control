<script>
  import Section from "./Section.svelte";
  import Card from "../ui/Card.svelte";
  import Button from "../ui/Button.svelte";
  import StatusDot from "../ui/StatusDot.svelte";
  import EmptyState from "../ui/EmptyState.svelte";
  import Fact from "../ui/Fact.svelte";
  import SettingRow from "../SettingRow.svelte";
  import TxCard from "../TxCard.svelte";
  import DevicePicto from "../DevicePicto.svelte";
  import AccessIssueCard from "../AccessIssueCard.svelte";
  import { devices as store } from "../store.svelte.js";

  /**
   * What's connected, how it's doing, and the one setting people actually
   * come here to flip. Everything else lives one click away in a group
   * section — the overview's job is to answer "is it working" in a glance,
   * not to be a second copy of every control.
   */
  let { icon = null, onnavigate } = $props();

  // A refresh that turns its own glyph reads as "this action, in progress";
  // one that swaps the glyph for a spinner reads as "something else now".
  let refreshing = $state(false);
  async function refresh() {
    refreshing = true;
    try {
      await store.refresh();
    } finally {
      refreshing = false;
    }
  }

  const device = $derived(store.device);
  const status = $derived(store.status);
  // Noise cancelling is the reason this app gets opened; it earns the one
  // shortcut on this screen.
  const quick = $derived(
    ["noise-cancel-power", "noise-cancel"]
      .map((id) => store.settingsById[id])
      .filter(Boolean),
  );
</script>

<Section
  title="概览"
  {icon}
  subtitle={device ? (status?.rx?.serial ?? device.id) : "未连接设备"}
>
  {#snippet actions()}
    <Button
      variant="ghost"
      icon="refresh"
      spin={refreshing}
      onclick={refresh}
      title="立即刷新 (Ctrl+R)"
    >
      刷新
    </Button>
  {/snippet}

  {#if store.accessIssue}
    <AccessIssueCard />
  {:else if !device}
    <Card>
      <EmptyState
        icon="plug"
        title="未连接麦克风"
        description="请通过 USB 连接受支持的大疆麦克风。连接后本页会自动显示它的状态。"
      />
    </Card>
  {:else}
    <Card>
      <div class="rx">
        <span class="picto"><DevicePicto pictogram={`${device.pictogram_key}-rx`} size={44} /></span>
        <div class="rx-titles">
          <h2>{device.model_name}</h2>
          <p class="u-caption">接收器</p>
        </div>
        <StatusDot
          tone={status?.connected ? "ok" : "warn"}
          text={status?.connected ? "已连接" : "无信号"}
        />
      </div>

      <dl class="u-facts facts">
        <Fact label="序列号" icon="tag" clamp>{status?.rx?.serial ?? "未知"}</Fact>
        <Fact label="固件" icon="chip" clamp>{status?.rx?.firmware ?? "未知"}</Fact>
        <Fact label="协议" icon="layers">
          {status?.protocol_version ? `v${status.protocol_version}` : "识别中"}
        </Fact>
      </dl>
    </Card>

    {#if (status?.tx ?? []).length > 0}
      <div class="tx-grid">
        {#each status.tx as tx, i (i)}
          <TxCard {tx} index={i} />
        {/each}
      </div>
    {/if}

    {#if quick.length > 0}
      <Card title="降噪" icon="wave" subtitle="最常用的一项，其余设置在左侧分组中。">
        {#snippet actions()}
          <!-- `iconEnd`, not an <Icon> inside the label: a block-level SVG in
               an inline label span forces its own line, which is what put this
               chevron on a second row. -->
          <Button
            variant="ghost"
            iconEnd="chevron-right"
            onclick={() => onnavigate?.("group:音频")}
          >
            全部音频设置
          </Button>
        {/snippet}
        {#each quick as setting (setting.id)}
          <SettingRow
            {setting}
            value={store.values[setting.id] ?? null}
            state={store.writeState(setting.id)}
            lockReason={store.lockReason(setting)}
            onchange={(id, value) => store.change(id, value)}
          />
        {/each}
      </Card>
    {/if}
  {/if}
</Section>

<style>
  .rx {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }

  .picto {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    flex: 0 0 auto;
    color: var(--text-tertiary);
  }

  .rx-titles {
    flex: 1 1 auto;
    min-width: 0;
  }

  /* Rides on .u-facts (app.css); the rule above it is the only thing specific
     to this card — the one-line clamp on long serials now comes from Fact's
     own `clamp`, so the two cards that need it ask for it the same way. */
  .facts {
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
  }

  .tx-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: var(--space-4);
  }
</style>
