<script>
  import Card from "./ui/Card.svelte";
  import Button from "./ui/Button.svelte";
  import CloseButton from "./ui/CloseButton.svelte";
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

  // A `<dialog>` takes no accessible name from its contents, so without this it
  // announces as a bare "dialog" — the same gap ui/Dialog.svelte closes.
  const id = $props.id();
  const titleId = `${id}-title`;

  /** How long the copy button stays on 已复制. Long enough to be read as an
   *  answer, short enough that the button is back to offering the action
   *  before anyone reaches for it again. */
  const COPIED_MS = 1600;

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
      setTimeout(() => (copied = false), COPIED_MS);
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

<dialog bind:this={dialog} class="udev" aria-labelledby={titleId}>
  {#if help}
    <header>
      <h2 id={titleId}>启用 USB 访问权限</h2>
      <CloseButton label="关闭 USB 访问权限说明" onclick={() => dialog.close()} />
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
  /* The scrim and the entrance animation come from app.css's `dialog` rules.
     This panel used to hand-roll the first and simply not have the second, so
     it appeared instantly while the app's other modal materialized. */

  .udev header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: var(--space-4);
  }

  /* Leading stays at the app's caption value and the air goes *between* the
     steps instead. A 1.8 line-height put as much space inside a wrapped step
     as between two steps, so a three-line instruction read as three
     instructions — the one thing a numbered list must not do. */
  .steps {
    margin: 0 0 var(--space-5);
    padding-left: var(--space-5);
    line-height: var(--type-caption-line);
    font-size: var(--type-caption-size);
  }
  .steps li + li {
    margin-top: var(--space-2);
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
  /* `min-width: 0` is what makes the ellipsis work at all. A flex child's
     automatic minimum size is its content, so this `<code>` refused to shrink
     below the full rules-file path and pushed the copy button out of the header
     instead of truncating — the ellipsis was decoration on a box that never
     got narrow enough to use it. `overflow-wrap: normal` opts back out of the
     global `code` rule (app.css): every other `<code>` in the app must wrap
     rather than clip, but this one truncates by design and a wrapping path
     would grow the header instead. */
  .rule-head code {
    min-width: 0;
    overflow: hidden;
    overflow-wrap: normal;
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
    line-height: var(--type-caption-line);
  }

  .foot {
    margin-top: var(--space-4);
  }
</style>
