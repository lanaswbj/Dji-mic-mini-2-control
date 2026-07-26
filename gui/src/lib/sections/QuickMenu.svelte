<script>
  import Section from "./Section.svelte";
  import Card from "../ui/Card.svelte";
  import Icon from "../ui/Icon.svelte";
  import { appInfo, pieMenuSlots } from "../api.js";

  /**
   * The pie menu, finally visible from the main window.
   *
   * Until now this feature existed only as a global hotkey and a ~1000-line
   * backend module: there was nothing anywhere in the app that told you the
   * overlay existed, what the shortcut was, or what any of its six slots did.
   * A capability the user can't discover is a capability they don't have.
   *
   * Both the hotkey string and the slot list come from the backend — the same
   * constants the code that runs them is compiled against — so this page
   * cannot describe a menu the app doesn't actually have.
   */
  let { icon = null } = $props();

  let slots = $state([]);
  // The literal here is only what's on screen for the one frame before the
  // backend answers; `HOTKEY_LABEL` in pie_menu.rs is the real value.
  let hotkey = $state("Ctrl + Alt + P");

  $effect(() => {
    pieMenuSlots()
      .then((v) => (slots = v))
      .catch(() => (slots = []));
    appInfo()
      .then((v) => (hotkey = v.pie_menu_hotkey))
      .catch(() => {});
  });
</script>

<Section title="快捷菜单" {icon} subtitle="悬浮在任务栏上方的扇形菜单，作用于当前正在使用的窗口。">
  <Card title="如何打开" icon="keyboard">
    <div class="keys">
      <kbd>{hotkey}</kbd>
      <span class="u-caption">全局有效，在任何应用中都能呼出</span>
    </div>
    <ul class="how">
      <li class="u-icon-line">
        <Icon name="keyboard" size="sm" /><span>方向键移动选中项，回车确认，Esc 取消</span>
      </li>
      <li class="u-icon-line">
        <Icon name="tap" size="sm" /><span>轻敲麦克风外壳同样可以移动选中项</span>
      </li>
      <li class="u-icon-line">
        <Icon name="mic" size="sm" /><span>按接收器配对键等同于回车，可直接确认</span>
      </li>
    </ul>
  </Card>

  <Card title="菜单项" icon="pie" subtitle="确认后，动作会发送给呼出菜单前你正在使用的那个窗口。">
    <ol class="slots">
      {#each slots as slot (slot.index)}
        <li>
          <span class="mark"><Icon name={slot.icon} size="sm" /></span>
          <span class="text">
            <span class="label">{slot.label}</span>
            <span class="u-caption">{slot.effect}</span>
          </span>
        </li>
      {/each}
      {#if slots.length === 0}
        <li class="u-caption">读取菜单项失败——快捷菜单在当前平台上不可用。</li>
      {/if}
    </ol>
  </Card>

  <Card title="Claude Code 提问" icon="question">
    <p class="u-caption u-measure">
      当 Claude Code 需要你做出选择（授权某个操作，或回答一个单选问题）时，快捷菜单会自动弹出并显示真实的问题与选项，选完即回答，不需要切回终端窗口。
      详细的联动开关和端口说明在<strong>偏好设置</strong>中。
    </p>
  </Card>
</Section>

<style>
  .keys {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  kbd {
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border-strong);
    border-bottom-width: 2px;
    border-radius: var(--radius-sm);
    background: var(--surface-sunken);
    font-family: var(--font-ui);
    font-size: var(--type-caption-size);
    font-weight: 600;
    white-space: nowrap;
  }

  .how,
  .slots {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  /* `.u-icon-line` (app.css) supplies the layout — these lines wrap on a
     narrow window, and centring the glyph against a two-line paragraph parks
     it halfway down the text. Only the type scale is local. */
  .how li {
    font-size: var(--type-caption-size);
    color: var(--text-secondary);
  }

  .slots li {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
  }

  .mark {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    flex: 0 0 auto;
    border-radius: var(--radius-sm);
    background: var(--accent-soft);
    color: var(--accent);
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: var(--space-05);
    min-width: 0;
  }

  .label {
    font-size: var(--type-body-size);
    font-weight: 550;
  }
</style>
