<script>
  import Card from "./ui/Card.svelte";
  import Button from "./ui/Button.svelte";
  import Icon from "./ui/Icon.svelte";
  import { toast } from "./ui/toasts.svelte.js";
  import { devices as store } from "./store.svelte.js";
  import { installUsbDriver, udevHelp } from "./api.js";

  /**
   * Shown when a supported microphone is on the USB bus but the app can't
   * open it — a missing WinUSB driver on Windows, a missing udev rule on
   * Linux. This is the single most important thing the window can say when
   * it applies, so it gets a real card with the fix attached rather than the
   * old build's clickable banner wedged into document flow above everything.
   */
  let busy = $state(false);
  let help = $state(null);
  let dialog = $state(null);
  let copied = $state(false);

  async function fixDriver() {
    if (busy) return;
    busy = true;
    try {
      await installUsbDriver();
      await store.refresh();
    } catch (e) {
      toast.error("驱动安装未完成", { detail: String(e) });
    } finally {
      busy = false;
    }
  }

  async function openUdev() {
    try {
      help ??= await udevHelp();
      dialog?.showModal();
    } catch (e) {
      toast.error("无法读取 udev 规则", { detail: String(e) });
    }
  }

  async function copyRule() {
    try {
      await navigator.clipboard.writeText(help.rule);
      copied = true;
      setTimeout(() => (copied = false), 1600);
    } catch {
      toast.warn("复制失败，请手动选中规则文本");
    }
  }
</script>

<Card
  tone="warn"
  icon="plug"
  title={store.snap?.os === "windows" ? "需要安装 USB 驱动" : "需要 USB 访问权限"}
  subtitle="已检测到受支持的麦克风，但应用还无法与它通信。"
>
  {#if store.snap?.os === "windows"}
    <p class="u-caption u-measure">
      点击下方按钮会下载 DJI 官方签名的驱动安装工具 Zadig 并以管理员身份启动。在弹出的窗口中，从下拉列表选择本设备（型号名中含
      <code>Interface 6</code> 或 <code>MI_06</code>），确认驱动类型为 WinUSB，然后点击 Install
      Driver。安装完成后工具会自动关闭并被本应用删除。
    </p>
    <div class="actions">
      <Button variant="primary" icon="download" {busy} onclick={fixDriver}>
        {busy ? "正在下载安装向导…" : "一键修复驱动"}
      </Button>
    </div>
  {:else}
    <p class="u-caption u-measure">
      在 Linux 上，USB 设备需要一条 udev 规则才能授予当前用户访问权限。
    </p>
    <div class="actions">
      <Button variant="primary" onclick={openUdev}>查看设置步骤</Button>
    </div>
  {/if}
</Card>

<dialog bind:this={dialog} class="udev">
  {#if help}
    <header>
      <h2>启用 USB 访问权限</h2>
      <button class="close" onclick={() => dialog.close()} aria-label="关闭">
        <Icon name="x" size="sm" />
      </button>
    </header>
    <ol class="steps">
      {#each help.steps as step, i (i)}<li>{step}</li>{/each}
    </ol>
    <div class="rule">
      <div class="rule-head">
        <code class="u-caption">{help.file}</code>
        <Button variant="ghost" icon={copied ? "check" : "copy"} onclick={copyRule}>
          {copied ? "已复制" : "复制规则"}
        </Button>
      </div>
      <pre>{help.rule}</pre>
    </div>
    <p class="u-caption foot">Linux 的 .deb 与 .rpm 安装包会自动安装这条规则。</p>
  {/if}
</dialog>

<style>
  .actions {
    display: flex;
    gap: var(--space-2);
  }

  code {
    font-family: var(--font-mono);
  }

  .udev {
    width: min(600px, calc(100vw - 2 * var(--space-8)));
    max-height: 84vh;
    padding: var(--space-6);
    border: 1px solid var(--border);
    border-radius: var(--radius-xl);
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--elev-4), var(--edge-highlight);
  }
  .udev::backdrop {
    background: var(--scrim);
    backdrop-filter: blur(2px);
  }

  .udev header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-4);
  }
  .close {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-secondary);
  }
  .close:hover {
    background: var(--surface-sunken);
    color: var(--text);
  }

  .steps {
    margin: 0 0 var(--space-5);
    padding-left: var(--space-5);
    line-height: 1.8;
    font-size: var(--type-caption-size);
  }

  .rule {
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-sunken);
  }
  .rule-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-1) var(--space-1) var(--space-1) var(--space-3);
    border-bottom: 1px solid var(--border);
  }
  .rule-head code {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
  }
  pre {
    margin: 0;
    padding: var(--space-3);
    overflow-x: auto;
    font-family: var(--font-mono);
    font-size: var(--type-caption-size);
    line-height: 1.5;
  }

  .foot {
    margin-top: var(--space-4);
  }
</style>
