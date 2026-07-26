<script>
  import Section from "./Section.svelte";
  import Card from "../ui/Card.svelte";
  import Button from "../ui/Button.svelte";
  import Dialog from "../ui/Dialog.svelte";
  import StatusDot from "../ui/StatusDot.svelte";
  import Fact from "../ui/Fact.svelte";
  import Icon from "../ui/Icon.svelte";
  import { toast } from "../ui/toasts.svelte.js";
  import {
    pairingButtonTestActive,
    micTapTestStatus,
    micTapReportFalsePositive,
    micTapReportFalseNegative,
    micTapTrainingStatus,
    micTapRollbackModel,
    micTapRestoreFactoryModel,
  } from "../api.js";

  /**
   * The two hardware gestures the app listens for, what they do, and the
   * controls for teaching the tap detector when it gets one wrong.
   *
   * The old tab was called 接收器快捷键 — named after `shortcut.rs`, a module
   * that is a permanent stub and always reports unavailable on Windows. It
   * showed two unexplained blinking dots and never said what either gesture
   * was actually *for*. Both gestures are wired to the quick menu; that's the
   * first thing this screen says now.
   */
  let { active = true, icon = null } = $props();

  let pairing = $state(false);
  let tap = $state({ count: 0, active: false, deviceFound: false });
  let training = $state({
    state: "idle",
    modelSource: "",
    modelTrainedAtUnixMs: 0,
    modelTrainingRows: 0,
    modelConfidenceThreshold: 0,
    feedbackRowCount: 0,
    canRollback: false,
    lastMessage: "",
  });

  let busy = $state(false);
  let confirmFactory = $state(false);

  const SOURCE_LABEL = {
    Embedded: "出厂模型",
    FullRetrain: "本地全量训练",
    Incremental: "增量训练更新",
  };
  const source = $derived(SOURCE_LABEL[training.modelSource] ?? "未知");
  const isTraining = $derived(training.state === "training");
  const trainedAt = $derived(
    training.modelTrainedAtUnixMs
      ? new Date(training.modelTrainedAtUnixMs).toLocaleString()
      : null,
  );

  /** Run `fn`, turn its outcome into a toast, then re-read the status. */
  async function run(fn, okTitle, okDetail) {
    busy = true;
    try {
      await fn();
      toast.success(okTitle, { detail: okDetail });
    } catch (e) {
      toast.error("操作未完成", { detail: String(e) });
    } finally {
      busy = false;
      pollTraining();
    }
  }

  async function pollTraining() {
    try {
      training = await micTapTrainingStatus();
    } catch {
      /* non-Windows, or the tap watcher hasn't started yet */
    }
  }

  // Live indicators only make sense while you can see them, so the fast poll
  // runs only while this section is on screen and the window is visible.
  $effect(() => {
    if (!active) return;
    let cancelled = false;
    const tick = async () => {
      // Independently guarded: the tap watcher not finding an audio device
      // must not stop the pairing-button indicator from updating.
      try {
        const v = await pairingButtonTestActive();
        if (!cancelled) pairing = v;
      } catch {
        /* non-Windows */
      }
      try {
        const v = await micTapTestStatus();
        if (!cancelled) tap = v;
      } catch {
        /* non-Windows */
      }
    };
    tick();
    const timer = setInterval(tick, 150);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  });

  $effect(() => {
    if (!active) return;
    pollTraining();
    const timer = setInterval(pollTraining, 2000);
    return () => clearInterval(timer);
  });
</script>

<Section title="敲击与按键" {icon} subtitle="接收器配对键与麦克风外壳敲击，都是快捷菜单的输入方式。">
  <Card title="配对键" icon="corner-down-left" subtitle="按下接收器上的配对键，等同于按下回车键。">
    <p class="u-caption u-measure">
      快捷菜单打开时，它用来确认当前选中的项；语音输入进行中时，它用来结束这次输入。
    </p>
    <div class="probe" class:hot={pairing}>
      <StatusDot
        tone={pairing ? "ok" : "neutral"}
        text={pairing ? "检测到按下" : "等待按下配对键…"}
        pulse={pairing}
      />
    </div>
  </Card>

  <Card title="麦克风敲击" icon="tap" subtitle="轻敲发射器外壳 1 下或 2 下，用来移动快捷菜单的选中项。">
    {#if !tap.deviceFound}
      <p class="u-caption u-icon-line warn-line">
        <Icon name="alert" size="sm" /><span>未找到麦克风音频输入设备，请确认接收器已连接。</span>
      </p>
    {/if}
    <div class="taps">
      {#each [1, 2] as n (n)}
        <div class="probe" class:hot={tap.active && tap.count === n}>
          <StatusDot
            tone={tap.active && tap.count === n ? "ok" : "neutral"}
            text={`${n} 下`}
            pulse={tap.active && tap.count === n}
          />
        </div>
      {/each}
    </div>
    <p class="u-caption u-measure">连敲 3 下及以上会按 2 下处理。说话时的爆破音会被语音检测挡掉，不会误触发。</p>
  </Card>

  <Card title="识别反馈" icon="target" subtitle="识别错了就点一下，应用会在本地用你的反馈继续训练模型。">
    <p class="u-caption u-measure">
      反馈会立刻记录当时的声音特征，并在后台做一次小幅增量训练——不需要重启，也不会上传任何音频。新模型必须先通过一次「不会让日常噪声更容易误触发」的检查才会启用。
    </p>
    <div class="pair">
      <Button
        variant="secondary"
        icon="x"
        disabled={!tap.active || busy}
        title={tap.active ? "" : "只能撤销最近几百毫秒内刚发生的一次识别"}
        onclick={() => run(micTapReportFalsePositive, "已记录这次误判", "模型会在后台继续学习。")}
      >
        刚才不是敲击
      </Button>
      <Button
        variant="secondary"
        icon="plus"
        disabled={busy}
        onclick={() => run(micTapReportFalseNegative, "已记录这次漏判", "模型会在后台继续学习。")}
      >
        刚才敲了却没反应
      </Button>
    </div>
  </Card>

  <Card title="当前模型" icon="sparkle">
    {#snippet actions()}
      <StatusDot
        tone={isTraining ? "accent" : "neutral"}
        text={isTraining ? "正在增量训练" : "空闲"}
        pulse={isTraining}
      />
    {/snippet}
    <dl class="u-facts">
      <Fact label="来源" icon="box" mono={false}>{source}</Fact>
      <Fact label="训练样本" icon="hash">{training.modelTrainingRows}</Fact>
      <Fact label="待学习反馈" icon="target">{training.feedbackRowCount}</Fact>
      {#if trainedAt}
        <Fact label="最近更新" icon="clock">{trainedAt}</Fact>
      {/if}
    </dl>
    {#if training.lastMessage}
      <p class="u-caption u-measure">{training.lastMessage}</p>
    {/if}
    <div class="pair">
      <Button
        variant="secondary"
        icon="rollback"
        disabled={!training.canRollback || busy}
        title={training.canRollback ? "" : "还没有可回滚的上一个模型"}
        onclick={() => run(micTapRollbackModel, "已回滚到上一个模型")}
      >
        回滚上一次更新
      </Button>
      <Button variant="danger" icon="trash" disabled={busy} onclick={() => (confirmFactory = true)}>
        恢复出厂模型
      </Button>
    </div>
  </Card>
</Section>

<Dialog
  open={confirmFactory}
  title="恢复出厂模型？"
  description="这会丢弃全部本地学习成果，敲击识别会退回本应用出厂时的表现。当前模型会先被备份，之后仍可回滚。"
  confirmLabel="恢复出厂模型"
  {busy}
  oncancel={() => (confirmFactory = false)}
  onconfirm={() => {
    confirmFactory = false;
    run(micTapRestoreFactoryModel, "已恢复出厂模型", "之前的增量学习效果已清除。");
  }}
/>

<style>
  /* Layout comes from `.u-icon-line` (app.css) — this sentence wraps on a
     narrow window, and the glyph belongs on its first line. */
  .warn-line {
    color: var(--warn);
  }

  /* A live probe: the border lights up together with the dot, so the signal
     is carried by two things at once rather than by one small color patch. */
  .probe {
    display: flex;
    align-items: center;
    min-height: 44px;
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-sunken);
    transition: border-color var(--dur-fast) var(--ease-out),
      background var(--dur-fast) var(--ease-out);
  }
  .probe.hot {
    border-color: color-mix(in srgb, var(--ok) 55%, var(--border));
    background: var(--ok-soft);
  }

  .taps {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-3);
  }

  .pair {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
</style>
