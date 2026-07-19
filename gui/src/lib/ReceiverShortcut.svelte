<script>
  let {
    pairingTestActive = false,
    tapStatus = { count: 0, active: false, deviceFound: false },
  } = $props();
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
      {#each [1, 2, 3] as n}
        <div class="tap-cell">
          <span class="test-dot" class:on={tapStatus.active && tapStatus.count === n} aria-hidden="true"></span>
          <span>{n} 下</span>
        </div>
      {/each}
    </div>
    <p class="hint">轻敲麦克风外壳 1/2/3 下试试，对应的指示灯会亮起。</p>
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
    grid-template-columns: repeat(3, 1fr);
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
