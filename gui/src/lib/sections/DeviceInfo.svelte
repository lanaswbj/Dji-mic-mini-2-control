<script>
  import Section from "./Section.svelte";
  import Card from "../ui/Card.svelte";
  import Fact from "../ui/Fact.svelte";
  import EmptyState from "../ui/EmptyState.svelte";
  import StatusDot from "../ui/StatusDot.svelte";
  import AccessIssueCard from "../AccessIssueCard.svelte";
  import { devices as store } from "../store.svelte.js";

  /**
   * Identity and firmware, for the two moments this actually matters: filing
   * a bug, and working out why a setting is greyed out. The protocol version
   * gets a plain-language line, because "v1" on its own explains nothing to
   * someone wondering where half their settings went.
   */
  let { icon = null } = $props();

  const status = $derived(store.status);
  const device = $derived(store.device);
  const version = $derived(status?.protocol_version);

  const unknown = (v) => v ?? "未知";
</script>

<Section title="设备信息" {icon} subtitle={device?.model_name ?? "未连接设备"}>
  {#if store.accessIssue}
    <AccessIssueCard />
  {:else if !device}
    <Card>
      <EmptyState
        icon="box"
        title="未连接设备"
        description="连接麦克风后，这里会显示接收器与发射器的序列号和固件版本。"
      />
    </Card>
  {:else}
    <Card title="接收器" icon="chip">
      {#snippet actions()}
        <StatusDot
          tone={status?.connected ? "ok" : "warn"}
          text={status?.connected ? "已连接" : "无信号"}
        />
      {/snippet}
      <dl class="u-facts facts">
        <Fact label="型号" icon="box" mono={false}>{device.model_name}</Fact>
        <Fact label="序列号" icon="tag">{unknown(status?.rx?.serial)}</Fact>
        <Fact label="固件" icon="chip">{unknown(status?.rx?.firmware)}</Fact>
        <Fact label="设备标识" icon="hash">{device.id}</Fact>
      </dl>
    </Card>

    <Card title="通信协议" icon="layers">
      <dl class="u-facts facts">
        <Fact label="版本" icon="layers">{version ? `v${version}` : "识别中"}</Fact>
        {#if status?.gain_dial != null}
          <Fact label="增益旋钮" icon="sliders">{status.gain_dial}</Fact>
        {/if}
      </dl>
      <p class="u-caption u-measure">
        {#if version === 1}
          该固件使用 v1 协议，部分设置（例如降噪开关、发射器自动关机、音色）没有对应的写入命令，因此在本应用中会显示为锁定。升级固件后即可使用。
        {:else if version === 2}
          该固件使用 v2 协议，本应用支持的全部设置都可写入。
        {:else}
          协议版本由心跳数据自动识别，通常在连接后一两秒内确定。
        {/if}
      </p>
    </Card>

    {#each status?.tx ?? [] as tx, i (i)}
      <Card title={`发射器 ${i + 1}`} icon="mic" subtitle={tx?.product_name ?? undefined}>
        {#if tx}
          <dl class="u-facts facts">
            <Fact label="序列号" icon="tag">{unknown(tx.serial)}</Fact>
            <Fact label="固件" icon="chip">{unknown(tx.firmware)}</Fact>
          </dl>
        {:else}
          <p class="u-caption">此插槽当前没有连接发射器。</p>
        {/if}
      </Card>
    {/each}
  {/if}
</Section>

<style>
  /* Wider columns than .u-facts' default: every value on this screen is an
     identifier long enough to look broken when it wraps mid-string. */
  .facts {
    --fact-min: 160px;
  }
</style>
