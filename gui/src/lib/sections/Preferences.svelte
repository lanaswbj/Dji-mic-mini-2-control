<script>
  import Section from "./Section.svelte";
  import Card from "../ui/Card.svelte";
  import Row from "../ui/Row.svelte";
  import Fact from "../ui/Fact.svelte";
  import Switch from "../ui/Switch.svelte";
  import Segmented from "../ui/Segmented.svelte";
  import Icon from "../ui/Icon.svelte";
  import { toast } from "../ui/toasts.svelte.js";
  import { theme, THEMES } from "../theme.svelte.js";
  import { glass } from "../glass.svelte.js";
  import { appInfo, setAutostart } from "../api.js";

  /**
   * Everything about the app itself.
   *
   * Three of these had no in-window presence at all before: appearance
   * (the app followed the system and offered no override), autostart and
   * close-to-tray (tray-menu items only), and the Claude Code integration —
   * which binds two loopback ports and paints four undocumented colors onto
   * the tray icon, with nothing anywhere explaining either. Two open sockets
   * and a color code the user can't look up are a trust problem, not just a
   * discoverability one, so both are written down here in full.
   */
  let { icon = null } = $props();

  let info = $state(null);
  let autostartBusy = $state(false);

  const OS_LABEL = { windows: "Windows", macos: "macOS", linux: "Linux" };

  // The tray badge's colors, kept in the same order as
  // gui/src-tauri/src/claude_status.rs's ClaudeStatus enum. These are the
  // only literal colors in the whole front-end, and deliberately so: they
  // aren't styling, they're a copy of the exact pixels main.rs paints into
  // the icon, so a theme token here would make the legend describe a color
  // the tray isn't actually showing.
  const TRAY_LEGEND = [
    { color: null, label: "空闲", hint: "不绘制任何角标" },
    { color: "#2b7de0", label: "思考中", hint: "正在推理" },
    { color: "#e09a2b", label: "执行中", hint: "正在调用工具" },
    { color: "#2bb34a", label: "等待你", hint: "有问题或授权请求待回答" },
    { color: "#e02b2b", label: "出错", hint: "最近一次事件报告了错误" },
  ];

  $effect(() => {
    appInfo()
      .then((v) => (info = v))
      .catch((e) => toast.error("无法读取应用信息", { detail: String(e) }));
  });

  async function toggleAutostart(on) {
    autostartBusy = true;
    try {
      await setAutostart(on);
      info = { ...info, autostart: on };
    } catch (e) {
      toast.error("无法修改开机自启", { detail: String(e) });
    } finally {
      autostartBusy = false;
    }
  }
</script>

<Section title="偏好设置" {icon} subtitle="这些设置属于本应用，不会写入麦克风。">
  <Card title="外观" icon="palette">
    <!-- `moon`, not a half-filled contrast disc: at 16px a stroked circle with
         a diameter through it is indistinguishable from `info`, and this row
         sits two cards away from a real one. -->
    <Row
      label="主题"
      icon="moon"
      description="深浅色配色方案。跟随系统时会随系统设置实时切换。"
    >
      {#snippet control()}
        <Segmented
          options={THEMES}
          value={theme.value}
          label="主题"
          onchange={(v) => theme.set(v)}
        />
      {/snippet}
    </Row>
    <Row
      label="窗口毛玻璃"
      icon="layers"
      description="让标题栏、侧栏和背景透出桌面（Windows 11 的 Mica 材质）。关闭后窗口完全不透明。"
    >
      {#snippet control()}
        <Switch
          checked={glass.enabled}
          label="窗口毛玻璃"
          onchange={(on) => glass.set(on)}
        />
      {/snippet}
    </Row>
  </Card>

  <Card title="启动与窗口" icon="window">
    <Row
      label="开机时自动启动"
      icon="power"
      description="登录系统后在后台启动，托盘图标会持续显示麦克风状态。"
      state={autostartBusy ? "writing" : "idle"}
    >
      {#snippet control()}
        <Switch
          checked={!!info?.autostart}
          disabled={!info || autostartBusy}
          label="开机时自动启动"
          onchange={toggleAutostart}
        />
      {/snippet}
    </Row>
    <p class="u-caption u-measure u-icon-line">
      <Icon name="tray" size="sm" />
      <span>
        关闭窗口只会把应用收进托盘，不会退出——这样敲击检测和快捷菜单才能继续工作。要真正退出，请右键托盘图标选择「退出」。
      </span>
    </p>
  </Card>

  <Card title="托盘图标" icon="tray">
    <p class="u-caption u-measure">
      图标右下角显示麦克风的连接与电量状态；左上角的小圆点表示 Claude Code 的当前状态：
    </p>
    <ul class="legend">
      {#each TRAY_LEGEND as item (item.label)}
        <li>
          <span class="swatch" class:none={!item.color} style:background={item.color ?? "transparent"}
          ></span>
          <span class="text">
            <span class="label">{item.label}</span>
            <span class="u-caption">{item.hint}</span>
          </span>
        </li>
      {/each}
    </ul>
  </Card>

  <Card title="Claude Code 联动" icon="link">
    <p class="u-caption u-measure">
      本应用会在本机回环地址上开启两个监听端口，用来接收 Claude Code 的钩子事件——授权请求和单选问题会显示在快捷菜单里，其余事件用来更新托盘状态。两个端口都只绑定
      <code>127.0.0.1</code>，不接受来自网络的连接，也不会向外发送任何数据。
    </p>
    <dl class="u-facts facts">
      <Fact label="事件端口" icon="link">127.0.0.1:{info?.hook_port ?? "—"}</Fact>
      <Fact label="授权端口" icon="shield">127.0.0.1:{info?.permission_port ?? "—"}</Fact>
    </dl>
    <p class="u-caption u-measure u-icon-line">
      <Icon name="info" size="sm" />
      <span>
        需要联动时，还要在 <code>~/.claude/settings.json</code> 中注册对应的钩子；未注册时这两个端口只是空闲监听，不会有任何事件到达。
      </span>
    </p>
  </Card>

  <Card title="关于" icon="info">
    <dl class="u-facts facts">
      <Fact label="版本" icon="tag">{info?.version ?? "—"}</Fact>
      <Fact label="系统" icon="monitor" mono={false}>
        {OS_LABEL[info?.os] ?? info?.os ?? "—"}
      </Fact>
      <Fact label="快捷菜单热键" icon="keyboard">{info?.pie_menu_hotkey ?? "—"}</Fact>
    </dl>
  </Card>
</Section>

<style>
  code {
    font-family: var(--font-mono);
  }

  .legend {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  /* Top-aligned, not centred: each entry is a label above a hint, and a dot
     centred against two lines sits next to neither of them. */
  .legend li {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
  }

  /* The ring is what keeps a swatch visible whatever color it carries — the
     colors themselves are the tray badge's real pixels, so they can't be
     theme tokens (see TRAY_LEGEND). */
  .swatch {
    width: 14px;
    height: 14px;
    flex: 0 0 auto;
    /* Optically on the label's line rather than flush with the block top. */
    margin-top: 3px;
    border-radius: 50%;
    box-shadow: var(--swatch-ring);
  }
  .swatch.none {
    box-shadow: inset 0 0 0 1px var(--border-strong);
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: var(--space-05);
    min-width: 0;
  }
  .label {
    font-size: var(--type-caption-size);
    font-weight: 600;
  }

  /* Wider columns than .u-facts' default: an address:port pair reads as one
     token and shouldn't be broken across lines. */
  .facts {
    --fact-min: 180px;
  }
</style>
