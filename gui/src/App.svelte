<script>
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Sidebar from "./lib/Sidebar.svelte";
  import DevicePanel from "./lib/DevicePanel.svelte";
  import UdevModal from "./lib/UdevModal.svelte";
  import ReceiverShortcut from "./lib/ReceiverShortcut.svelte";
  import {
    snapshot,
    setSetting,
    setTxSetting,
    udevHelp,
    installUsbDriver,
    pairingButtonTestActive,
    micTapTestStatus,
  } from "./lib/api.js";

  // Custom window chrome. macOS keeps its native traffic lights (via the
  // Overlay title bar); Windows/Linux use our own frameless controls.
  const appWindow = getCurrentWindow();
  const isMac = /Mac/i.test(navigator.userAgent || navigator.platform || "");
  let maximized = $state(false);
  async function refreshMax() {
    try {
      maximized = await appWindow.isMaximized();
    } catch {
      /* ignore */
    }
  }
  const minimizeWin = () => appWindow.minimize();
  const toggleMaxWin = () => appWindow.toggleMaximize();
  const closeWin = () => appWindow.close();

  // Frameless windows lose the compositor's edge-resize borders, so we add our
  // own invisible handles (Linux/Windows; macOS keeps its native resize).
  const startResize = (dir) => (e) => {
    if (e.button === 0) appWindow.startResizeDragging(dir);
  };

  let snap = $state(null);
  let selected = $state(null);
  // Distinguishes "nothing selected yet" (auto-select the first device once
  // one shows up) from "the user deliberately cleared the selection" (leave
  // it cleared until they pick one again).
  let userDeselected = $state(false);
  let sidebarOpen = $state(true);
  let pending = $state({});
  let optimistic = $state({}); // values shown immediately, before the device confirms
  // Per-TX settings need their own optimistic values because the same setting
  // can differ between transmitter slots.
  let pendingTx = $state({});
  let optimisticTx = $state({});
  let view = $state("compact");
  let error = $state(null);
  let stableStatus = $state(null);
  let refreshInFlight = false;
  let workspace = $state("mic");
  let pairingTestActive = $state(false);
  let tapStatus = $state({ count: 0, active: false, deviceFound: false });

  let showUdev = $state(false);
  let help = $state(null);

  const devices = $derived(snap?.devices ?? []);
  const selectedDevice = $derived(devices.find((d) => d.id === selected) ?? null);
  const rawStatus = $derived(selectedDevice ? stableStatus ?? snap?.status ?? null : null);
  // Merge live values with any not-yet-confirmed local changes for instant feedback.
  const values = $derived.by(() => {
    const base = { ...(rawStatus?.settings ?? {}) };
    const txNc = (rawStatus?.tx ?? [])
      .map((tx, index) => applyOptimisticTx(tx, index))
      .filter(Boolean)
      .map((tx) => tx.nc_enabled)
      .filter((value) => typeof value === "boolean");
    if (txNc.length > 0) {
      base["noise-cancel-power"] = txNc.every(Boolean)
        ? "on"
        : txNc.every((value) => !value)
          ? "off"
          : "mixed";
    }
    return { ...base, ...optimistic };
  });
  function txKey(tx, settingId) {
    return `${tx}:${settingId}`;
  }
  function applyOptimisticTx(tx, index) {
    if (!tx) return tx;
    const next = { ...tx };
    const voiceTone = optimisticTx[txKey(index, "voice-tone")];
    const ncPower = optimisticTx[txKey(index, "noise-cancel-power")];
    const ncMode = optimisticTx[txKey(index, "noise-cancel")];
    if (voiceTone !== undefined) next.voice_tone = voiceTone;
    if (ncPower !== undefined) next.nc_enabled = ncPower === "on";
    if (ncMode !== undefined) next.nc_mode = ncMode;
    return next;
  }
  const status = $derived(
    rawStatus && { ...rawStatus, tx: rawStatus.tx.map((tx, i) => applyOptimisticTx(tx, i)) },
  );
  // Set on both Linux (missing udev rule) and Windows (missing WinUSB
  // driver on the receiver's control interface) when a matching device is
  // on the bus but couldn't be opened.
  const accessIssue = $derived(snap?.probe?.permission_issue && devices.length === 0);
  let driverBusy = $state(false);
  let driverError = $state(null);

  function mergeTx(prev, next) {
    if (!next) return null;
    if (!prev) return next;
    return {
      ...next,
      serial: next.serial ?? prev.serial,
      firmware: next.firmware ?? prev.firmware,
      product_name: next.product_name ?? prev.product_name,
      voice_tone: next.voice_tone ?? prev.voice_tone,
      charging: next.charging ?? prev.charging,
      battery: next.battery ?? prev.battery,
      nc_enabled: next.nc_enabled ?? prev.nc_enabled,
      nc_mode: next.nc_mode ?? prev.nc_mode,
      low_cut: next.low_cut ?? prev.low_cut,
      mic_leds: next.mic_leds ?? prev.mic_leds,
      auto_off: next.auto_off ?? prev.auto_off,
      nc_button: next.nc_button ?? prev.nc_button,
    };
  }

  function mergeRx(prev, next) {
    if (!next) return prev ?? null;
    if (!prev) return next;
    return {
      serial: next.serial ?? prev.serial,
      firmware: next.firmware ?? prev.firmware,
    };
  }

  function mergeStatus(prev, next) {
    if (!next) return null;
    if (!prev || prev.model_id !== next.model_id) return next;
    const nextSettings = next.settings ?? {};
    const prevSettings = prev.settings ?? {};
    return {
      ...next,
      nc_enabled:
        Object.keys(nextSettings).length === 0 && Object.keys(prevSettings).length > 0
          ? prev.nc_enabled
          : next.nc_enabled,
      rx: mergeRx(prev.rx, next.rx),
      tx: next.tx.map((tx, i) => mergeTx(prev.tx?.[i], tx)),
      settings:
        Object.keys(nextSettings).length === 0
          ? prevSettings
          : { ...prevSettings, ...nextSettings },
      protocol_version: next.protocol_version ?? prev.protocol_version,
      gain_dial: next.gain_dial ?? prev.gain_dial,
    };
  }

  async function refresh() {
    if (refreshInFlight) return;
    refreshInFlight = true;
    try {
      const next = await snapshot(selected);
      snap = next;
      // Auto-select once devices show up, unless the user deliberately
      // cleared the selection (clicking empty space in the sidebar).
      if (!selected && !userDeselected && next.devices.length > 0) {
        selected = next.devices[0].id;
      }
      // Drop a stale selection.
      if (selected && !next.devices.some((d) => d.id === selected)) {
        selected = next.devices[0]?.id ?? null;
        stableStatus = null;
      }
      stableStatus = next.status ? mergeStatus(stableStatus, next.status) : null;
      // Retire optimistic values the device has now confirmed.
      const confirmed = next.status?.settings ?? {};
      let changed = false;
      const remaining = { ...optimistic };
      for (const k of Object.keys(remaining)) {
        const txNc = (next.status?.tx ?? [])
          .filter(Boolean)
          .map((tx) => tx.nc_enabled)
          .filter((value) => typeof value === "boolean");
        const txNcConfirmed =
          k === "noise-cancel-power" &&
          txNc.length > 0 &&
          txNc.every((value) => value === (remaining[k] === "on"));
        if (confirmed[k] === remaining[k] && (k !== "noise-cancel-power" || txNc.length === 0 || txNcConfirmed)) {
          delete remaining[k];
          changed = true;
        }
      }
      if (changed) optimistic = remaining;

      // Keep each local TX value until that exact slot confirms it. An older
      // polling response must not make both controls appear synchronized.
      const confirmedTx = next.status?.tx ?? [];
      let changedTx = false;
      const remainingTx = { ...optimisticTx };
      for (const key of Object.keys(remainingTx)) {
        const [txText, settingId] = key.split(":");
        const confirmedTxValue = confirmedTx[Number(txText)];
        const expected = remainingTx[key];
        // The receiver mirrors NC power/mode into both TX records, even after
        // a targeted write. Keep those per-TX values as session truth; only
        // Voice Tone currently has an independently confirmable return value.
        const matches = settingId === "voice-tone" && confirmedTxValue?.voice_tone === expected;
        if (matches) {
          delete remainingTx[key];
          changedTx = true;
        }
      }
      if (changedTx) optimisticTx = remainingTx;
    } catch (e) {
      error = String(e);
    } finally {
      refreshInFlight = false;
    }
  }

  function select(id) {
    selected = id;
    userDeselected = id === null;
    refresh();
  }

  async function change(settingId, value) {
    if (!selected) return;
    if (settingId === "noise-cancel-power" || settingId === "noise-cancel") {
      optimisticTx = Object.fromEntries(
        Object.entries(optimisticTx).filter(([key]) => !key.endsWith(`:${settingId}`)),
      );
    }
    optimistic = { ...optimistic, [settingId]: value }; // reflect the flip instantly
    pending = { ...pending, [settingId]: true };
    error = null;
    try {
      await setSetting(selected, settingId, value);
      await refresh();
    } catch (e) {
      error = String(e);
      const { [settingId]: _drop, ...rest } = optimistic;
      optimistic = rest;
    } finally {
      const { [settingId]: _p, ...rest } = pending;
      pending = rest;
    }
  }

  async function changeTx(tx, settingId, value) {
    if (!selected) return;
    const key = txKey(tx, settingId);
    let nextOptimisticTx = { ...optimisticTx };
    if (settingId === "noise-cancel-power" || settingId === "noise-cancel") {
      // Snapshot both visible slots before the targeted write. The next raw
      // status packet mirrors the changed value to both records and cannot be
      // used to reconstruct the untouched transmitter.
      for (const [index, item] of (status?.tx ?? []).entries()) {
        if (!item) continue;
        const slotKey = txKey(index, settingId);
        if (nextOptimisticTx[slotKey] !== undefined) continue;
        const current =
          settingId === "noise-cancel-power"
            ? item.nc_enabled == null
              ? undefined
              : item.nc_enabled
                ? "on"
                : "off"
            : item.nc_mode;
        if (current !== undefined && current !== null) nextOptimisticTx[slotKey] = current;
      }
    }
    optimisticTx = { ...nextOptimisticTx, [key]: value };
    pendingTx = { ...pendingTx, [tx]: true };
    error = null;
    try {
      await setTxSetting(selected, tx, settingId, value);
      await refresh();
    } catch (e) {
      error = String(e);
      const { [key]: _drop, ...rest } = optimisticTx;
      optimisticTx = rest;
    } finally {
      const { [tx]: _p, ...rest } = pendingTx;
      pendingTx = rest;
    }
  }

  async function openUdev() {
    if (!help) help = await udevHelp();
    showUdev = true;
  }

  async function fixDriver() {
    if (driverBusy) return;
    driverBusy = true;
    driverError = null;
    try {
      await installUsbDriver();
      await refresh();
    } catch (e) {
      driverError = String(e);
    } finally {
      driverBusy = false;
    }
  }

  $effect(() => {
    refresh();
    const timer = setInterval(refresh, 250);
    return () => clearInterval(timer);
  });

  // Poll for the pairing-button press indicator and the mic-tap test status
  // while the shortcut panel is visible (see gui/src-tauri/src/pairing_button.rs
  // and mic_tap.rs).
  $effect(() => {
    if (workspace !== "shortcut") return;
    let cancelled = false;
    const tick = async () => {
      // Independent try/catch per call: one failing (e.g. the mic-tap
      // audio device not being present yet) must not stop the other from
      // updating.
      try {
        const active = await pairingButtonTestActive();
        if (!cancelled) pairingTestActive = active;
      } catch {
        // ignore — non-Windows
      }
      try {
        const tap = await micTapTestStatus();
        if (!cancelled) tapStatus = tap;
      } catch {
        // ignore — non-Windows
      }
    };
    tick();
    const timer = setInterval(tick, 150);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  });

  // Keep the maximize/restore icon in sync with the window state.
  $effect(() => {
    refreshMax();
    const onResize = () => refreshMax();
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  });
</script>

<div class="app" class:mac={isMac}>
  {#if !isMac}
    <div class="rz rz-n" onmousedown={startResize("North")}></div>
    <div class="rz rz-s" onmousedown={startResize("South")}></div>
    <div class="rz rz-e" onmousedown={startResize("East")}></div>
    <div class="rz rz-w" onmousedown={startResize("West")}></div>
    <div class="rz rz-ne" onmousedown={startResize("NorthEast")}></div>
    <div class="rz rz-nw" onmousedown={startResize("NorthWest")}></div>
    <div class="rz rz-se" onmousedown={startResize("SouthEast")}></div>
    <div class="rz rz-sw" onmousedown={startResize("SouthWest")}></div>
  {/if}

  <div class="topbar" class:mac={isMac} data-tauri-drag-region>
    <button class="icon-btn" onclick={() => (sidebarOpen = !sidebarOpen)} aria-label="显示或隐藏设备列表">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none">
        <path d="M3.5 6h17M3.5 12h17M3.5 18h17" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" />
      </svg>
    </button>
    <div class="workspace-switch" role="tablist" aria-label="工作区">
      <button class:active={workspace === "mic"} onclick={() => (workspace = "mic")} role="tab" aria-selected={workspace === "mic"}>麦克风</button>
      <button class:active={workspace === "shortcut"} onclick={() => (workspace = "shortcut")} role="tab" aria-selected={workspace === "shortcut"}>接收器快捷键</button>
    </div>
    <span class="brand" data-tauri-drag-region>{workspace === "shortcut" ? "接收器快捷键" : "大疆麦克风控制"}</span>
    <div class="drag-fill" data-tauri-drag-region></div>

    {#if !isMac}
      <div class="win-controls">
        <button class="win-btn" onclick={minimizeWin} aria-label="最小化">
          <svg width="11" height="11" viewBox="0 0 12 12"><path d="M2 6h8" stroke="currentColor" stroke-width="1.2" /></svg>
        </button>
        <button class="win-btn" onclick={toggleMaxWin} aria-label={maximized ? "还原" : "最大化"}>
          {#if maximized}
            <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.1">
              <rect x="3.2" y="1.6" width="6.2" height="6.2" rx="1" /><rect x="1.6" y="3.6" width="6.2" height="6.8" rx="1" fill="var(--bg-panel)" />
            </svg>
          {:else}
            <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.1"><rect x="2" y="2" width="8" height="8" rx="1" /></svg>
          {/if}
        </button>
        <button class="win-btn close" onclick={closeWin} aria-label="关闭">
          <svg width="11" height="11" viewBox="0 0 12 12"><path d="M2.5 2.5l7 7M9.5 2.5l-7 7" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" /></svg>
        </button>
      </div>
    {/if}
  </div>

  {#if accessIssue && snap?.os === "linux"}
    <button class="banner" onclick={openUdev}>
      已连接麦克风，但当前无权访问。点击修复 USB 权限。
    </button>
  {:else if accessIssue && snap?.os === "windows"}
    <button class="banner" onclick={fixDriver} disabled={driverBusy}>
      {driverBusy ? "正在下载驱动安装向导，请在权限提示中点击“是”，并在弹出的窗口中选择设备后点击 Install Driver…" : "已连接麦克风，但驱动未安装。点击一键修复（需要管理员权限）。"}
    </button>
  {/if}

  {#if driverError}
    <div class="banner err" role="alert">{driverError}</div>
  {/if}

  {#if error}
    <div class="banner err" role="alert">{error}</div>
  {/if}

  <div class="body">
    <div class="sidebar-wrap" class:open={sidebarOpen && workspace === "mic"}>
      {#if workspace === "mic"}<Sidebar {devices} {selected} onselect={select} />{/if}
    </div>

    <main class="main">
      {#if workspace === "shortcut"}
        <ReceiverShortcut {pairingTestActive} {tapStatus} />
      {:else if selectedDevice && status}
        <DevicePanel
          device={selectedDevice}
          {status}
          settings={snap.settings}
          {values}
          {pending}
          {pendingTx}
          {view}
          onchange={change}
          onchangeTx={changeTx}
          onview={(v) => (view = v)}
        />
      {:else}
        <div class="placeholder">
          <div class="ph-card">
            {#if accessIssue && snap?.os === "linux"}
              <h2>需要授权</h2>
              <p>已连接受支持的麦克风，但应用暂无访问权限。</p>
              <button class="cta" onclick={openUdev}>查看设置步骤</button>
            {:else if accessIssue && snap?.os === "windows"}
              <h2>需要安装驱动</h2>
              <p>
                已连接受支持的麦克风，但控制接口尚未安装驱动。点击下方按钮会下载官方驱动安装工具
                Zadig 并以管理员身份启动 —— 在弹出的窗口中从下拉列表选择本设备（型号名中含
                "Interface 6" 或 "MI_06" 字样），确认驱动类型为 WinUSB，然后点击 Install
                Driver。安装工具会在成功后自动关闭并被应用删除。
              </p>
              <button class="cta" onclick={fixDriver} disabled={driverBusy}>
                {driverBusy ? "正在下载安装向导…" : "一键修复驱动"}
              </button>
            {:else if devices.length > 0}
              <h2>未选择设备</h2>
              <p>请从侧边栏选择一个设备。</p>
            {:else}
              <h2>未连接麦克风</h2>
              <p>请通过 USB 连接受支持的大疆麦克风。</p>
            {/if}
          </div>
        </div>
      {/if}
    </main>
  </div>
</div>

{#if showUdev && help}
  <UdevModal {help} onclose={() => (showUdev = false)} />
{/if}

<style>
  .app {
    height: 100%;
    display: flex;
    flex-direction: column;
    /* The window is frameless on some platforms (no native border/shadow),
       so a solid-color desktop behind it can make the edge disappear
       entirely without this. Not needed on macOS, which already has native
       decorations (see .mac below). */
    border: 1px solid var(--border);
  }
  .app.mac {
    border: none;
  }
  /* Invisible resize handles pinned to the window edges/corners. */
  .rz {
    position: fixed;
    z-index: 100;
  }
  .rz-n {
    top: 0;
    left: 8px;
    right: 8px;
    height: 5px;
    cursor: ns-resize;
  }
  .rz-s {
    bottom: 0;
    left: 8px;
    right: 8px;
    height: 5px;
    cursor: ns-resize;
  }
  .rz-e {
    top: 8px;
    bottom: 8px;
    right: 0;
    width: 5px;
    cursor: ew-resize;
  }
  .rz-w {
    top: 8px;
    bottom: 8px;
    left: 0;
    width: 5px;
    cursor: ew-resize;
  }
  .rz-ne {
    top: 0;
    right: 0;
    width: 11px;
    height: 11px;
    cursor: nesw-resize;
  }
  .rz-nw {
    top: 0;
    left: 0;
    width: 11px;
    height: 11px;
    cursor: nwse-resize;
  }
  .rz-se {
    bottom: 0;
    right: 0;
    width: 11px;
    height: 11px;
    cursor: nwse-resize;
  }
  .rz-sw {
    bottom: 0;
    left: 0;
    width: 11px;
    height: 11px;
    cursor: nesw-resize;
  }
  .topbar {
    position: relative;
    flex: 0 0 auto;
    height: 30px;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 2px 0 4px;
    background: var(--bg-panel);
    border-bottom: 1px solid var(--border);
    user-select: none;
  }
  /* Leave room for the macOS traffic lights (top-left) under the overlay title bar. */
  .topbar.mac {
    padding-left: 76px;
  }
  .drag-fill {
    flex: 1 1 auto;
    align-self: stretch;
  }
  .icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    line-height: 0;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-dim);
  }
  .icon-btn:hover {
    background: var(--bg-elev);
    color: var(--text);
  }
  /* Kill inline-svg baseline gap so icons sit dead-center in their buttons. */
  .topbar button svg {
    display: block;
  }
  /* Absolutely centered so it stays put regardless of the side controls. */
  .brand {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    font-weight: 600;
    font-size: 13px;
    letter-spacing: 0.01em;
    color: var(--text-dim);
    white-space: nowrap;
  }
  .workspace-switch {
    display: flex;
    overflow: hidden;
    margin-left: 4px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-elev);
  }
  .workspace-switch button {
    height: 22px;
    padding: 0 9px;
    border: 0;
    border-right: 1px solid var(--border);
    background: transparent;
    color: var(--text-dim);
    font-size: 10px;
  }
  .workspace-switch button:last-child { border-right: 0; }
  .workspace-switch button.active {
    background: var(--bg-panel);
    color: var(--text);
    font-weight: 650;
  }
  .win-controls {
    display: flex;
    align-self: stretch;
    margin-left: 4px;
  }
  .win-btn {
    width: 40px;
    align-self: stretch;
    display: grid;
    place-items: center;
    border: none;
    background: transparent;
    color: var(--text-dim);
    transition: background 0.12s, color 0.12s;
  }
  .win-btn:hover {
    background: var(--bg-elev);
    color: var(--text);
  }
  .win-btn.close:hover {
    background: #e5484d;
    color: #fff;
  }
  .banner {
    flex: 0 0 auto;
    text-align: left;
    border: none;
    padding: 10px 16px;
    background: color-mix(in srgb, var(--warn) 16%, var(--bg-panel));
    color: var(--text);
    border-bottom: 1px solid color-mix(in srgb, var(--warn) 40%, transparent);
    font-size: 13px;
  }
  .banner.err {
    background: color-mix(in srgb, var(--danger) 16%, var(--bg-panel));
    border-bottom-color: color-mix(in srgb, var(--danger) 40%, transparent);
  }
  .body {
    flex: 1 1 auto;
    display: flex;
    min-height: 0;
  }
  /* Collapsible sidebar: animate width so the panel slides in/out. The inner
     Sidebar keeps a fixed width and is clipped, giving a smooth slide. */
  .sidebar-wrap {
    flex: 0 0 auto;
    width: 260px;
    overflow: hidden;
    border-right: 1px solid var(--border);
    transition: width 0.22s ease, border-color 0.22s ease;
  }
  .sidebar-wrap:not(.open) {
    width: 0;
    border-right-color: transparent;
  }
  .main {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
  }
  .placeholder {
    flex: 1 1 auto;
    display: grid;
    place-items: center;
    padding: 24px;
  }
  .ph-card {
    text-align: center;
    max-width: 340px;
    color: var(--text-dim);
  }
  .ph-card h2 {
    margin: 0 0 8px;
    color: var(--text);
    font-size: 18px;
  }
  .ph-card p {
    margin: 0 0 16px;
    line-height: 1.5;
  }
  .cta {
    border: 1px solid var(--accent);
    background: var(--accent);
    color: var(--accent-contrast);
    padding: 8px 16px;
    border-radius: var(--radius-sm);
    font-weight: 600;
  }
</style>
