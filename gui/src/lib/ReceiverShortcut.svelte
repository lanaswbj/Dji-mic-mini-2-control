<script>
  import {
    micTapReportFalsePositive,
    micTapReportFalseNegative,
    micTapTrainingStatus,
    micTapRollbackModel,
    micTapRestoreFactoryModel,
  } from "./api.js";

  let {
    pairingTestActive = false,
    tapStatus = { count: 0, active: false, deviceFound: false },
  } = $props();

  let fbBusy = $state(false);
  let fbMessage = $state("");
  let fbError = $state(false);
  let trainStatus = $state({
    state: "idle",
    modelSource: "",
    modelTrainedAtUnixMs: 0,
    modelTrainingRows: 0,
    modelConfidenceThreshold: 0,
    feedbackRowCount: 0,
    canRollback: false,
    lastMessage: "",
  });

  const SOURCE_LABEL = {
    Embedded: "出厂模型",
    FullRetrain: "本地全量训练",
    Incremental: "增量训练更新",
  };

  async function pollTrainStatus() {
    try {
      trainStatus = await micTapTrainingStatus();
    } catch {
      // ignore — non-Windows, or watcher not ready yet
    }
  }

  async function reportFalsePositive() {
    fbBusy = true;
    fbError = false;
    try {
      await micTapReportFalsePositive();
      fbMessage = "已记录这次误判，模型会在后台用它继续学习。";
    } catch (e) {
      fbError = true;
      fbMessage = String(e);
    } finally {
      fbBusy = false;
      pollTrainStatus();
    }
  }

  async function reportFalseNegative() {
    fbBusy = true;
    fbError = false;
    try {
      await micTapReportFalseNegative();
      fbMessage = "已记录这次漏判，模型会在后台用它继续学习。";
    } catch (e) {
      fbError = true;
      fbMessage = String(e);
    } finally {
      fbBusy = false;
      pollTrainStatus();
    }
  }

  async function rollback() {
    fbError = false;
    try {
      await micTapRollbackModel();
      fbMessage = "已回滚到上一个模型。";
    } catch (e) {
      fbError = true;
      fbMessage = String(e);
    } finally {
      pollTrainStatus();
    }
  }

  async function restoreFactory() {
    fbError = false;
    try {
      await micTapRestoreFactoryModel();
      fbMessage = "已恢复出厂模型，之前的增量学习效果已被清除。";
    } catch (e) {
      fbError = true;
      fbMessage = String(e);
    } finally {
      pollTrainStatus();
    }
  }

  $effect(() => {
    pollTrainStatus();
    const timer = setInterval(pollTrainStatus, 2000);
    return () => clearInterval(timer);
  });
</script>

<div class="shortcut-panel">
  <header class="shortcut-head-main">
    <div class="title-mark" aria-hidden="true">
      <svg viewBox="0 0 24 24" width="22" height="22" fill="none">
        <rect x="4" y="3" width="16" height="18" rx="3" stroke="currentColor" stroke-width="1.7" />
        <circle cx="12" cy="8" r="1.4" fill="currentColor" />
        <path d="M8.5 13h7M8.5 16.5h7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
      </svg>
    </div>
    <div>
      <h1>接收器快捷键</h1>
      <span class="engine-state">检测测试</span>
    </div>
  </header>

  <section class="shortcut-control" aria-label="配对键检测测试">
    <div class="shortcut-head">
      <span class="section-label">配对键检测测试</span>
    </div>
    <div class="test-row">
      <span class="test-dot" class:on={pairingTestActive} aria-hidden="true"></span>
      <span>{pairingTestActive ? "检测到按下" : "按下接收器的配对键试试"}</span>
    </div>
  </section>

  <section class="shortcut-control" aria-label="麦克风敲击检测测试">
    <div class="shortcut-head">
      <span class="section-label">麦克风敲击检测（测试）</span>
    </div>
    {#if !tapStatus.deviceFound}
      <div class="empty">未找到麦克风音频输入设备，请确认接收器已连接。</div>
    {/if}
    <div class="tap-row">
      {#each [1, 2] as n}
        <div class="tap-cell">
          <span class="test-dot" class:on={tapStatus.active && tapStatus.count === n} aria-hidden="true"></span>
          <span>{n} 下</span>
        </div>
      {/each}
    </div>
    <p class="hint">轻敲麦克风外壳 1/2 下试试，对应的指示灯会亮起（3 下及以上会算作 2 下）。</p>
  </section>

  <section class="shortcut-control" aria-label="识别反馈与增量训练">
    <div class="shortcut-head">
      <span class="section-label">识别反馈 · 增量训练</span>
    </div>
    <p class="hint">识别错了就点一下——应用会用你的反馈在本地自动继续训练模型，无需重启、无需重装。</p>
    <div class="feedback-row">
      <button
        class="fb-btn"
        disabled={!tapStatus.active || fbBusy}
        onclick={reportFalsePositive}
        title={!tapStatus.active ? "只能撤销最近几百毫秒内刚发生的一次识别" : ""}
      >
        刚才不是敲击（误判，撤销）
      </button>
      <button class="fb-btn" disabled={fbBusy} onclick={reportFalseNegative}>
        刚才敲了却没反应（漏判，补报）
      </button>
    </div>
    {#if fbMessage}
      <div class="fb-message" class:err={fbError}>{fbMessage}</div>
    {/if}

    <div class="train-status">
      <span>当前模型：{SOURCE_LABEL[trainStatus.modelSource] ?? "未知"}</span>
      <span>已积累反馈：{trainStatus.feedbackRowCount} 条</span>
      <span>状态：{trainStatus.state === "training" ? "正在增量训练…" : "空闲"}</span>
    </div>
    {#if trainStatus.lastMessage}
      <p class="hint">{trainStatus.lastMessage}</p>
    {/if}

    <div class="feedback-row">
      <button class="fb-btn ghost" disabled={!trainStatus.canRollback} onclick={rollback}>
        回滚到上一个模型
      </button>
      <button class="fb-btn ghost" onclick={restoreFactory}>恢复出厂模型</button>
    </div>
  </section>
</div>

<style>
  .shortcut-panel {
    flex: 1 1 auto;
    min-width: 0;
    overflow-y: auto;
    padding: 30px 34px 42px;
  }
  .shortcut-head-main {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 24px;
  }
  .title-mark {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: 8px;
    background: color-mix(in srgb, var(--good) 15%, var(--bg-panel));
    color: var(--good);
  }
  h1 {
    margin: 0;
    font-size: 21px;
    letter-spacing: 0;
  }
  .engine-state {
    color: var(--text-dim);
    font-size: 12px;
  }
  .empty {
    margin-bottom: 4px;
    padding: 10px 12px;
    border-left: 3px solid var(--border-strong);
    background: color-mix(in srgb, var(--text-dim) 8%, transparent);
    color: var(--text-dim);
    font-size: 12px;
  }
  .shortcut-control {
    display: grid;
    gap: 14px;
    padding-top: 22px;
    border-top: 1px solid var(--border);
  }
  .shortcut-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }
  .section-label {
    color: var(--text-dim);
    font-size: 12px;
    font-weight: 650;
  }
  .test-row {
    display: flex;
    align-items: center;
    gap: 10px;
    border: 1px solid var(--border);
    border-radius: 7px;
    padding: 11px 13px;
    background: var(--bg-panel);
    color: var(--text-dim);
    font-size: 12px;
  }
  .tap-row {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 10px;
  }
  .tap-cell {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    border: 1px solid var(--border);
    border-radius: 7px;
    padding: 11px 13px;
    background: var(--bg-panel);
    color: var(--text-dim);
    font-size: 12px;
  }
  .hint {
    margin: 0;
    color: var(--text-dim);
    font-size: 11px;
  }
  .feedback-row {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 10px;
  }
  .fb-btn {
    border: 1px solid var(--border);
    border-radius: 7px;
    padding: 10px 12px;
    background: var(--bg-panel);
    color: var(--text);
    font-size: 12px;
    font-weight: 600;
    text-align: center;
  }
  .fb-btn:hover:not(:disabled) {
    background: var(--bg-elev);
    border-color: var(--border-strong);
  }
  .fb-btn:disabled {
    color: var(--text-dim);
    opacity: 0.55;
  }
  .fb-btn.ghost {
    font-weight: 500;
    color: var(--text-dim);
  }
  .fb-message {
    padding: 8px 10px;
    border-radius: 6px;
    background: color-mix(in srgb, var(--good) 12%, var(--bg-panel));
    color: var(--text);
    font-size: 12px;
  }
  .fb-message.err {
    background: color-mix(in srgb, var(--danger) 14%, var(--bg-panel));
  }
  .train-status {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 16px;
    color: var(--text-dim);
    font-size: 11px;
  }
  .test-dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--border-strong);
    flex-shrink: 0;
    transition: background 0.1s ease, box-shadow 0.1s ease;
  }
  .test-dot.on {
    background: var(--good);
    box-shadow: 0 0 8px 1px color-mix(in srgb, var(--good) 60%, transparent);
  }
  @media (max-width: 760px) {
    .shortcut-panel { padding: 22px 18px 34px; }
  }
</style>
