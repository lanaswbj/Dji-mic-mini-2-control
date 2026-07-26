<script>
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { devices as store, TEMPO } from "./lib/store.svelte.js";
  import { theme } from "./lib/theme.svelte.js";
  import { glass } from "./lib/glass.svelte.js";
  import { APP_SECTIONS, deviceSections } from "./lib/nav.js";
  import DeviceSwitcher from "./lib/DeviceSwitcher.svelte";
  import Icon from "./lib/ui/Icon.svelte";
  import ToastStack from "./lib/ui/ToastStack.svelte";
  import { toast } from "./lib/ui/toasts.svelte.js";
  import Overview from "./lib/sections/Overview.svelte";
  import SettingsGroup from "./lib/sections/SettingsGroup.svelte";
  import DeviceInfo from "./lib/sections/DeviceInfo.svelte";
  import InputGestures from "./lib/sections/InputGestures.svelte";
  import QuickMenu from "./lib/sections/QuickMenu.svelte";
  import Preferences from "./lib/sections/Preferences.svelte";

  // --- Window chrome ----------------------------------------------------
  // macOS keeps its native traffic lights (overlay title bar); everywhere else
  // the window is frameless and draws its own controls and resize edges.
  const appWindow = getCurrentWindow();
  const isMac = /Mac/i.test(navigator.userAgent || navigator.platform || "");
  let maximized = $state(false);

  const startResize = (dir) => (e) => {
    if (e.button === 0) appWindow.startResizeDragging(dir);
  };
  /** Edge class suffix -> the direction name Tauri's resize API expects. */
  const RESIZE_EDGES = {
    n: "North", s: "South", e: "East", w: "West",
    ne: "NorthEast", nw: "NorthWest", se: "SouthEast", sw: "SouthWest",
  };

  // --- Navigation -------------------------------------------------------
  let section = $state("overview");
  let sidebarOpen = $state(true);

  const deviceNav = $derived(deviceSections(store.groups));
  const nav = $derived([
    { title: "设备", items: deviceNav },
    { title: "应用", items: APP_SECTIONS },
  ]);
  const flat = $derived(nav.flatMap((g) => g.items));
  const current = $derived(flat.find((s) => s.id === section) ?? flat[0]);

  // A model can stop declaring a group (unplugged, or a different model
  // selected) while its section is open. Fall back rather than render nothing.
  $effect(() => {
    if (!flat.some((s) => s.id === section)) section = "overview";
  });

  function go(id) {
    section = id;
    // Scroll position is per-section, not shared: arriving at a section
    // already scrolled halfway down is disorienting.
    contentEl?.scrollTo({ top: 0 });
  }

  // --- The selection marker ---------------------------------------------
  // One element that slides between nav items rather than a background color
  // hopping from one button to the next — the same reasoning as Segmented's
  // indicator, and the thing that makes the two items read as one control.
  // Measured rather than computed from an index, so it stays correct if a
  // label ever wraps to two lines or the user scales their system font.
  let navEl = $state(null);
  let marker = $state({ y: 0, h: 0 });
  let markerReady = $state(false);

  $effect(() => {
    // Re-measure whenever the selection or the set of destinations changes.
    // Runs after Svelte has flushed the DOM, so the active item is already
    // painted where it will stay.
    void section;
    void nav;
    const el = navEl?.querySelector(".nav-item.active");
    if (!el) {
      marker = { y: 0, h: 0 };
      markerReady = false;
      return;
    }
    marker = { y: el.offsetTop, h: el.offsetHeight };
    // The first placement must not animate in from the top of the list; every
    // move after it must. One frame is enough to commit the initial position.
    if (!markerReady) requestAnimationFrame(() => (markerReady = true));
  });

  // --- Polling ----------------------------------------------------------
  // The overview shows live level meters and needs the fast poll; nothing
  // else does. A hidden window (closed to tray) needs no poll at all — the
  // old build kept hammering the USB bus at 250ms forever after being closed.
  let visible = $state(!document.hidden);

  $effect(() => {
    const onVis = () => (visible = !document.hidden);
    document.addEventListener("visibilitychange", onVis);
    return () => document.removeEventListener("visibilitychange", onVis);
  });

  $effect(() => {
    if (!visible) store.setTempo(TEMPO.off);
    else store.setTempo(section === "overview" ? TEMPO.live : TEMPO.calm);
  });

  $effect(() => () => store.stop());

  // A transport-level failure isn't attributable to any one row, so it
  // surfaces as a toast rather than a banner that reflows the whole page.
  let lastError = $state(null);
  $effect(() => {
    const e = store.error;
    if (e && e !== lastError) toast.error("无法读取设备状态", { detail: e });
    lastError = e;
  });

  // --- Scroll edge ------------------------------------------------------
  // Apple's rule: a hard divider under floating chrome reads as a seam. The
  // separation should appear only once content is actually sliding under the
  // header, so the header publishes its state as an inherited custom property
  // and SectionHeader consumes it — no prop drilling through three components
  // for one boolean.
  let contentEl = $state(null);
  let scrolled = $state(false);

  // --- Keyboard ---------------------------------------------------------
  // Ctrl+1..9 jumps to a section, matching how every tabbed desktop app
  // behaves. Ctrl+B and Ctrl+, are the platform conventions for the sidebar
  // and preferences.
  function onKeydown(e) {
    if (!e.ctrlKey || e.altKey) return;
    if (e.key >= "1" && e.key <= "9") {
      const target = flat[Number(e.key) - 1];
      if (target) {
        e.preventDefault();
        go(target.id);
      }
    } else if (e.key === "b") {
      e.preventDefault();
      sidebarOpen = !sidebarOpen;
    } else if (e.key === ",") {
      e.preventDefault();
      go("prefs");
    } else if (e.key === "r") {
      e.preventDefault();
      store.refresh();
    }
  }

  $effect(() => {
    theme.apply();
    glass.apply();
    const sync = async () => {
      try {
        maximized = await appWindow.isMaximized();
      } catch {
        /* not fatal — the icon just keeps its last state */
      }
    };
    sync();
    window.addEventListener("resize", sync);
    return () => window.removeEventListener("resize", sync);
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="app" class:mac={isMac} class:maximized>
  {#if !isMac && !maximized}
    {#each Object.entries(RESIZE_EDGES) as [edge, dir] (edge)}
      <!-- Pointer-only chrome: the OS already offers keyboard window resizing
           through the system menu, so these are explicitly presentational
           rather than fake buttons cluttering the tab order. A maximized
           window has no edges to drag, and leaving them live there would put
           invisible strips over real controls for no purpose. -->
      <div class="rz rz-{edge}" role="presentation" onmousedown={startResize(dir)}></div>
    {/each}
  {/if}

  <header class="titlebar" class:mac={isMac} data-tauri-drag-region>
    <button
      class="chrome-btn"
      onclick={() => (sidebarOpen = !sidebarOpen)}
      aria-label="显示或隐藏侧边栏"
      aria-pressed={sidebarOpen}
      title="侧边栏 (Ctrl+B)"
    >
      <Icon name="sidebar" size="sm" />
    </button>

    <span class="brand" data-tauri-drag-region>
      <Icon name="mic" size="sm" />
      <span>大疆麦克风控制</span>
    </span>
    <div class="drag-fill" data-tauri-drag-region></div>

    {#if !isMac}
      <div class="win-controls">
        <button class="win-btn" onclick={() => appWindow.minimize()} aria-label="最小化">
          <Icon name="minimize" size="sm" />
        </button>
        <button
          class="win-btn"
          onclick={() => appWindow.toggleMaximize()}
          aria-label={maximized ? "还原" : "最大化"}
        >
          <Icon name={maximized ? "restore" : "maximize"} size="sm" />
        </button>
        <button class="win-btn close" onclick={() => appWindow.close()} aria-label="关闭到托盘">
          <Icon name="x" size="sm" />
        </button>
      </div>
    {/if}
  </header>

  <div class="body">
    <nav class="sidebar" class:open={sidebarOpen} aria-label="主导航">
      <div class="sidebar-inner">
        <div class="switcher">
          <DeviceSwitcher
            devices={store.devices}
            selected={store.selected}
            onselect={(id) => store.select(id)}
          />
        </div>

        <div class="groups" bind:this={navEl}>
          {#if marker.h > 0}
            <span
              class="marker"
              class:ready={markerReady}
              style:height="{marker.h}px"
              style:transform="translateY({marker.y}px)"
              aria-hidden="true"
            ></span>
          {/if}

          {#each nav as group (group.title)}
            <div class="group">
              <p class="u-label group-title">{group.title}</p>
              <ul>
                {#each group.items as item (item.id)}
                  <li>
                    <button
                      class="nav-item"
                      class:active={section === item.id}
                      aria-current={section === item.id ? "page" : undefined}
                      onclick={() => go(item.id)}
                    >
                      <Icon name={item.icon} size="sm" />
                      <span>{item.label}</span>
                    </button>
                  </li>
                {/each}
              </ul>
            </div>
          {/each}
        </div>
      </div>
    </nav>

    <main
      class="content"
      class:scrolled
      bind:this={contentEl}
      onscroll={(e) => (scrolled = e.currentTarget.scrollTop > 2)}
    >
      <!-- Keyed so navigating between two setting groups — which reuse the
           same component with a different prop — replays the section's entry
           motion instead of silently swapping its contents. -->
      {#key current.id}
        {#if current.id === "overview"}
          <Overview icon={current.icon} onnavigate={go} />
        {:else if current.id.startsWith("group:")}
          <SettingsGroup icon={current.icon} group={current.id.slice(6)} />
        {:else if current.id === "info"}
          <DeviceInfo icon={current.icon} />
        {:else if current.id === "input"}
          <InputGestures icon={current.icon} active={visible} />
        {:else if current.id === "pie"}
          <QuickMenu icon={current.icon} />
        {:else}
          <Preferences icon={current.icon} />
        {/if}
      {/key}
    </main>
  </div>
</div>

<ToastStack />

<style>
  .app {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  /* The window's own hairline edge, drawn as a pointer-transparent overlay
     rather than as a border on .app.

     As a border it inset the entire layout by 1px, which is precisely why the
     caption buttons could never fill the top-right corner: their hover fill
     stopped one pixel short of it on two sides. As an overlay the hairline
     crosses *over* the buttons — exactly what a real Windows frame line does —
     and the fill runs corner to corner underneath it.

     Frameless windows get no compositor border, so on a dark desktop this is
     the only thing separating a dark app from it. A maximized window has no
     outside to be separated from, so it goes away there. */
  .app::after {
    content: "";
    position: fixed;
    inset: 0;
    z-index: 400;
    pointer-events: none;
    box-shadow: inset 0 0 0 1px var(--border);
  }
  .app.mac::after,
  .app.maximized::after {
    content: none;
  }

  /* Invisible resize handles pinned to the window edges and corners. */
  .rz {
    position: fixed;
    z-index: 300;
  }
  .rz-n { top: 0; left: 8px; right: 8px; height: 5px; cursor: ns-resize; }
  .rz-s { bottom: 0; left: 8px; right: 8px; height: 5px; cursor: ns-resize; }
  .rz-e { top: 8px; bottom: 8px; right: 0; width: 5px; cursor: ew-resize; }
  .rz-w { top: 8px; bottom: 8px; left: 0; width: 5px; cursor: ew-resize; }
  .rz-ne { top: 0; right: 0; width: 11px; height: 11px; cursor: nesw-resize; }
  .rz-nw { top: 0; left: 0; width: 11px; height: 11px; cursor: nwse-resize; }
  .rz-se { bottom: 0; right: 0; width: 11px; height: 11px; cursor: nwse-resize; }
  .rz-sw { bottom: 0; left: 0; width: 11px; height: 11px; cursor: nesw-resize; }

  .titlebar {
    position: relative;
    z-index: 20;
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    height: var(--titlebar-h);
    padding-inline: var(--space-2) 0;
    background: var(--material-chrome);
    backdrop-filter: var(--blur-chrome);
    box-shadow: inset 0 -1px 0 var(--border);
    user-select: none;
  }
  /* Room for the macOS traffic lights under the overlay title bar. */
  .titlebar.mac {
    padding-left: 76px;
  }

  .drag-fill {
    flex: 1 1 auto;
    align-self: stretch;
  }

  /* Absolutely centered so it stays put no matter what flanks it. */
  .brand {
    position: absolute;
    left: 50%;
    top: 50%;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    transform: translate(-50%, -50%);
    font-size: var(--type-caption-size);
    font-weight: 600;
    letter-spacing: var(--type-caption-track);
    color: var(--text-secondary);
    white-space: nowrap;
    pointer-events: none;
  }

  /* The two button clusters sit above the resize handles (z-index 300).
     Those are fixed-position strips pinned to the window edges, and .rz-n /
     .rz-ne lay directly over the caption buttons' top edge and the top-right
     corner: a pointer inside that band never reached the button at all, so the
     hover fill visibly stopped short of the corner. Windows offers no resize
     over its caption buttons either, so yielding those pixels of grab area is
     the native behaviour rather than a compromise. */
  .chrome-btn,
  .win-controls {
    position: relative;
    z-index: 310;
  }

  .chrome-btn {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-secondary);
    transition: background var(--dur-fast) var(--ease-out),
      color var(--dur-fast) var(--ease-out),
      transform var(--dur-press) var(--ease-out);
  }
  .chrome-btn:hover {
    background: var(--surface-sunken);
    color: var(--text);
  }
  .chrome-btn:active {
    transform: scale(0.92);
  }
  .chrome-btn[aria-pressed="true"] {
    color: var(--text);
  }

  .win-controls {
    display: flex;
    align-self: stretch;
  }
  .win-btn {
    display: grid;
    place-items: center;
    width: 46px;
    align-self: stretch;
    border: none;
    border-radius: 0;
    background: none;
    color: var(--text-secondary);
    transition: background var(--dur-fast) var(--ease-out),
      color var(--dur-fast) var(--ease-out);
  }
  .win-btn:hover {
    background: var(--surface-sunken);
    color: var(--text);
  }
  /* No `transform` press feedback here, unlike every other button in the app:
     scaling a control whose whole job is to fill the window corner would
     un-fill it on the way down. The darker fill carries the press instead. */
  .win-btn:active {
    background: var(--border);
  }
  /* The Windows convention: the close button turns red on hover, in both
     themes, so its glyph is the un-themed white (see app.css) rather than a
     surface color that would follow the page and lose contrast. */
  .win-btn.close:hover {
    background: var(--danger);
    color: var(--fixed-white);
  }
  .win-btn.close:active {
    background: color-mix(in srgb, var(--danger) 82%, black);
    color: var(--fixed-white);
  }

  .body {
    flex: 1 1 auto;
    display: flex;
    min-height: 0;
  }

  /* Collapsing the sidebar is the one place this app animates a layout
     property, and it does so knowingly: the content column genuinely has to
     reflow to reclaim the space, and no transform-only trick avoids that
     without leaving the content the wrong width at one end of the animation.
     What *is* avoided is the expensive half — .sidebar-inner keeps its full
     width and slides, so the panel's own contents never reflow, and the panel
     reads as sliding out from behind the edge instead of being progressively
     unmasked in place. */
  .sidebar {
    flex: 0 0 auto;
    width: 0;
    overflow: hidden;
    background: var(--material-sidebar);
    backdrop-filter: var(--blur-sidebar);
    /* Inset, so toggling the edge never nudges the layout by a pixel the way
       a real border did. */
    box-shadow: inset -1px 0 0 var(--border);
    will-change: width;
    transition: width var(--dur-spring) var(--ease-spring);
  }
  .sidebar.open {
    width: var(--sidebar-w);
  }

  .sidebar-inner {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
    width: var(--sidebar-w);
    height: 100%;
    padding: var(--space-3) var(--space-3) var(--space-5);
    overflow-y: auto;
    transform: translateX(calc(-1 * var(--sidebar-w)));
    transition: transform var(--dur-spring) var(--ease-spring);
  }
  .sidebar.open .sidebar-inner {
    transform: translateX(0);
  }

  .switcher {
    flex: 0 0 auto;
  }

  .groups {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }

  /* The selection, as one object that moves. `.nav-item.active` deliberately
     carries no background of its own — if it did, two fills would be lit at
     once for the length of the slide. */
  .marker {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    border-radius: var(--radius-sm);
    background: var(--accent-soft);
    pointer-events: none;
  }
  .marker.ready {
    transition: transform var(--dur-spring) var(--ease-spring),
      height var(--dur-spring) var(--ease-spring);
  }

  .group-title {
    padding-inline: var(--space-3);
    margin-bottom: var(--space-2);
  }

  .sidebar ul {
    display: flex;
    flex-direction: column;
    gap: var(--space-05);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .nav-item {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    min-height: 34px;
    padding: var(--space-1) var(--space-3);
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-secondary);
    font-size: var(--type-caption-size);
    font-weight: 500;
    text-align: left;
    transition: background var(--dur-fast) var(--ease-out),
      color var(--dur-fast) var(--ease-out),
      transform var(--dur-press) var(--ease-out);
  }
  .nav-item:hover {
    background: var(--surface-sunken);
    color: var(--text);
  }
  .nav-item:active {
    transform: scale(0.98);
  }
  .nav-item.active,
  .nav-item.active:hover {
    background: none;
    color: var(--accent);
    font-weight: 600;
  }

  .content {
    flex: 1 1 auto;
    min-width: 0;
    overflow-y: auto;
    background: var(--material-content);
    /* Sections size themselves against the reading column, not the window.
       At the 760px minimum width an open sidebar leaves ~520px here, and a
       viewport media query cannot see that — which is how a setting row with
       a four-option segmented picker ended up squeezed at exactly the size it
       was supposed to stack at. */
    container: content / inline-size;
    --scroll-edge: 0;
  }
  .content.scrolled {
    --scroll-edge: 1;
  }

  /* Below this width the sidebar would leave the content unusably narrow, so
     it overlays instead of pushing. */
  @media (max-width: 720px) {
    .sidebar.open {
      position: absolute;
      top: var(--titlebar-h);
      bottom: 0;
      z-index: 15;
      box-shadow: inset -1px 0 0 var(--border), var(--elev-3);
    }
  }
</style>
