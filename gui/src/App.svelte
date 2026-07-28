<script>
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { devices as store, TEMPO } from "./lib/store.svelte.js";
  import { theme } from "./lib/theme.svelte.js";
  import { glass } from "./lib/glass.svelte.js";
  import { fluidScroll } from "./lib/fluidScroll.js";
  import { APP_SECTIONS, deviceSections } from "./lib/nav.js";
  import DeviceSwitcher from "./lib/DeviceSwitcher.svelte";
  import Dock from "./lib/Dock.svelte";
  import Icon from "./lib/ui/Icon.svelte";
  import ToastStack from "./lib/ui/ToastStack.svelte";
  import { toast } from "./lib/ui/toasts.svelte.js";
  import Overview from "./lib/sections/Overview.svelte";
  import SettingsGroup from "./lib/sections/SettingsGroup.svelte";
  import DeviceInfo from "./lib/sections/DeviceInfo.svelte";
  import InputGestures from "./lib/sections/InputGestures.svelte";
  import QuickMenu from "./lib/sections/QuickMenu.svelte";
  import Preferences from "./lib/sections/Preferences.svelte";

  /**
   * The window's shape: one solid title bar, one full-width content plane, and
   * the navigation floating over it as a dock.
   *
   * The sidebar this replaced was spending a fixed quarter of the window on
   * seven labels that never change, and it forced every section to lay itself
   * out against a column whose width depended on a toggle. With it gone the
   * content plane is the window, the reading measure is the only thing deciding
   * how wide a section gets, and the one piece of chrome left is the piece that
   * genuinely floats.
   */

  // --- Window chrome ----------------------------------------------------
  // macOS keeps its native traffic lights (overlay title bar); everywhere else
  // the window is frameless and draws its own controls and resize edges.
  const appWindow = getCurrentWindow();
  const isMac = /Mac/i.test(navigator.userAgent || navigator.platform || "");
  let maximized = $state(false);

  const startResize = (dir) => (e) => {
    if (e.button === 0) appWindow.startResizeDragging(dir);
  };
  /** Edge class suffix -> the direction name Tauri's resize API expects.
   *
   *  No `ne`: the top-right corner is where the caption buttons live, and
   *  Windows offers no resize grip over its own caption buttons either. An
   *  11x11 handle there sat on top of the close button and ate its hover — see
   *  the .rz / .win-controls note in the styles below. NorthEast stays
   *  reachable by grabbing the north or east edge beside it. */
  const RESIZE_EDGES = {
    n: "North", s: "South", e: "East", w: "West",
    nw: "NorthWest", se: "SouthEast", sw: "SouthWest",
  };

  // --- Navigation -------------------------------------------------------
  // One flat list. The 设备/应用 tiers the sidebar drew as two captioned groups
  // are gone with it — a dock has no room for headings, and with at most seven
  // destinations there is nothing left for a heading to disambiguate.
  let section = $state("overview");

  const nav = $derived([...deviceSections(store.groups), ...APP_SECTIONS]);
  const current = $derived(nav.find((s) => s.id === section) ?? nav[0]);

  // A model can stop declaring a group (unplugged, or a different model
  // selected) while its section is open. Fall back rather than render nothing.
  $effect(() => {
    if (!nav.some((s) => s.id === section)) section = "overview";
  });

  function go(id) {
    section = id;
    // Scroll position is per-section, not shared: arriving at a section
    // already scrolled halfway down is disorienting.
    contentEl?.scrollTo({ top: 0 });
    // And focus follows, so the arrow keys can actually scroll the thing that
    // just arrived. `.content` is the app's only scroll container, and the
    // browser only scrolls the nearest scrollable ancestor *of the focused
    // element* — the dock's buttons are its siblings, not its descendants, so
    // clicking a dock item used to leave the keyboard unable to move the page
    // at all. 设备信息 made it total: that section contains no focusable
    // element whatsoever, so its lower cards were unreachable without a mouse.
    // `preventScroll` because we just set the position deliberately.
    contentEl?.focus({ preventScroll: true });
  }

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
  // behaves — and it follows the *user's* order, since that is the order on
  // screen. Ctrl+, is the platform convention for preferences.
  function onKeydown(e) {
    // A modal dialog must own the keyboard while it is up. `showModal()` inerts
    // the background for pointers and assistive tech, but keydown still bubbles
    // to `window` — so with the 恢复出厂模型 confirmation open, Ctrl+3 navigated
    // the page behind the scrim and Ctrl+R re-polled, leaving the user
    // confirming a destructive action against a screen that had changed under
    // it. Queried live rather than tracked as state: `<dialog>` is the single
    // source of truth for its own openness, and nothing has to remember to
    // register.
    if (document.querySelector("dialog[open]")) return;
    if (!e.ctrlKey || e.altKey) return;
    if (e.key >= "1" && e.key <= "9") {
      const target = nav[Number(e.key) - 1];
      if (target) {
        e.preventDefault();
        go(target.id);
      }
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
    <!-- Which receiver every 设备 section is talking about. It used to sit at
         the top of the sidebar; a scope selector belongs in the chrome, not in
         the navigation, and this is the only place left that is chrome. -->
    <div class="scope">
      <DeviceSwitcher
        compact
        devices={store.devices}
        selected={store.selected}
        onselect={(id) => store.select(id)}
      />
    </div>

    <span class="brand" data-tauri-drag-region>
      <Icon name="mic" size="sm" />
      <span>DJI Mic Control</span>
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
    <!-- The clip layer exists so the content plane can be *moved*: a rubber
         band excursion translates .content, and without a parent holding the
         material still the bounce would expose a strip of bare window. It is
         also the dock's positioning context, so the dock stays put while the
         plane under it bounces. -->
    <div class="content-clip">
      <!-- `tabindex="-1"` makes the scroll container itself focusable so the
           arrow keys and PageUp/PageDown have something to act on — see `go()`.
           Not a control and never a tab stop; it is only ever focused
           programmatically. -->
      <main
        class="content"
        class:scrolled
        tabindex="-1"
        bind:this={contentEl}
        use:fluidScroll
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

      <Dock items={nav} current={current.id} onnavigate={go} />
    </div>
  </div>
</div>

<ToastStack />

<style>
  .app {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    /* The caption cluster's geometry, single-sourced. `.rz-n` has to stop
       exactly where the cluster starts (see the .rz block) and `.win-btn` has
       to be exactly that wide — the same number was previously held in two
       places with nothing tying them together, so shrinking a button would have
       silently re-covered the close button with a resize strip.
       Declared on .app rather than on .titlebar because the .rz handles are
       .titlebar's *siblings* and could not inherit it from there.
       46px is Windows' own caption-button metric (46x32); these stretch to
       --titlebar-h rather than to 32. */
    --caption-btn-w: 46px;
    --caption-w: calc(3 * var(--caption-btn-w));
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
    /* Follows the window's own arc. Square, this hairline cut diagonally
       across the rounded top-right corner instead of hugging it — so the close
       button's red hover fill appeared to stop short of the edge, with a step
       of border colour between the red and the real corner. `box-shadow`
       respects `border-radius`, so one declaration fixes both corners. */
    border-radius: var(--window-radius);
    box-shadow: inset 0 0 0 1px var(--border);
  }
  .app.mac::after,
  .app.maximized::after {
    content: none;
  }

  /* Invisible resize handles pinned to the window edges and corners.
     Deliberately carved *around* the caption cluster rather than layered under
     it.

     The previous attempt gave .win-controls `z-index: 310` against these
     handles' 300 and documented itself as fixed. It never worked, and could not:
     `.titlebar` is `position: relative; z-index: 20`, which makes it a stacking
     context, so 310 only ranks its children *inside* that context — the whole
     title bar still competes with these handles at 20 against 300 and loses.
     `.rz-ne` (the corner), `.rz-n` (the top 5px) and `.rz-e` (the right 5px)
     therefore covered the caption buttons: a pointer in that band hit a handle,
     `:hover` never fired, and the close button's red fill vanished exactly when
     the pointer reached the corner it is supposed to fill.

     Shrinking the strips fixes it at the source, with no z-index race to win
     and nothing lost: Windows offers no resize over its own caption buttons
     either. `.rz-ne` is gone entirely (and with it its RESIZE_EDGES entry —
     leaving the CSS out alone would ship a styleless div). */
  .rz {
    position: fixed;
    z-index: 300;
  }
  .rz-n { top: 0; left: 8px; right: var(--caption-w); height: 5px; cursor: ns-resize; }
  .rz-s { bottom: 0; left: 8px; right: 8px; height: 5px; cursor: ns-resize; }
  .rz-e { top: var(--titlebar-h); bottom: 8px; right: 0; width: 5px; cursor: ew-resize; }
  .rz-w { top: 8px; bottom: 8px; left: 0; width: 5px; cursor: ew-resize; }
  .rz-nw { top: 0; left: 0; width: 11px; height: 11px; cursor: nwse-resize; }
  .rz-se { bottom: 0; right: 0; width: 11px; height: 11px; cursor: nwse-resize; }
  .rz-sw { bottom: 0; left: 0; width: 11px; height: 11px; cursor: nesw-resize; }

  .titlebar {
    position: relative;
    /* Deliberately **no** `z-index`, so this does not become a stacking context.
       It used to be `z-index: 20`, and that was the thing standing between the
       caption buttons and the window frame hairline: a stacking context traps
       its children's z-indices inside it, so `.win-controls` could never rank
       against `.app::after` (400) whatever number it was given — the whole title
       bar competed as one box at 20 and lost. With `z-index: auto` the caption
       cluster ranks at the root and can finally sit above the hairline.
       Nothing is lost by dropping it: `.titlebar` is still positioned, so it
       paints above the non-positioned flow, and it does not overlap
       `.content-clip` at all (they are flex siblings). The one child that has to
       outrank the content plane — `.scope`, whose popover hangs down over it —
       carries its own z-index and now applies it at the root. */
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: var(--titlebar-h);
    padding-inline: var(--space-2) 0;
    /* Opaque, and deliberately not a material.
       This strip used to be unpainted window, showing the Acrylic backdrop
       directly. That is a defensible way to prove the glass is on, and it read
       as a hole: a band of desktop above the app, with the app's own name
       floating in it and no surface under the buttons. Requested to be solid
       instead, so it is a plate — --surface, which is pure white in the light
       theme and the matching near-black in the dark one, in both cases the same
       colour a card is. The glass is still visible, in the gutters around the
       content plane below, where it frames the app instead of interrupting it.
       Unaffected by the 外观 → 窗口毛玻璃 toggle by design: this is the one
       surface whose whole job is to *not* be translucent. */
    background: var(--surface);
    /* The plate follows the window's own corner; the caption buttons inside it
       do not (no `overflow: hidden`, so `border-radius` clips only this
       background). That asymmetry is the point. DWM rounds the composited
       window to --window-radius, and whatever is painted inside that arc is
       what the anti-aliased edge blends. With a square plate the topmost pixels
       of the corner were this white --surface, so the close button's red fill
       still read as stopping a hair short of the edge even once it was painting
       above the frame hairline. Rounding the plate and leaving the button's
       fill square means red is the last thing under the arc, which is what
       Windows' own caption button looks like. */
    border-radius: var(--window-radius) var(--window-radius) 0 0;
    /* Only on the underside — this is a plate sitting on the window, and a
       hairline all the way round would draw a box nobody asked for. */
    box-shadow: inset 0 -1px 0 var(--border);
    user-select: none;
  }
  /* A maximized window has square corners, so the plate must too — otherwise
     two notches of desktop show through the top corners. */
  .app.maximized .titlebar {
    border-radius: 0;
  }
  /* Room for the macOS traffic lights under the overlay title bar. */
  .titlebar.mac {
    padding-left: 76px;
  }

  /* Capped so a long model name can never push the window controls around;
     DeviceSwitcher ellipsises inside it. */
  .scope {
    position: relative;
    z-index: 310;
    flex: 0 1 auto;
    min-width: 0;
    max-width: 260px;
  }

  .drag-fill {
    flex: 1 1 auto;
    align-self: stretch;
  }

  /* Absolutely centred so it stays put no matter what flanks it. */
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

  /* Above `.app::after` (400), which is the window's frame hairline.
     That hairline is drawn as a full-window inset box-shadow, so it ran across
     the top and right edges of the caption buttons — leaving a 1px line of
     --border between the close button's red hover fill and the actual window
     edge. Windows' own close button fills its corner completely; the frame line
     does not survive on top of it. This is the *only* thing allowed above the
     hairline, and it works only because `.titlebar` no longer creates a stacking
     context (see there).
     Separately from painting: the resize handles are carved *around* this
     cluster rather than layered under it, because a z-index race is the wrong
     tool for hit-testing and losing it was the original bug. See the .rz block. */
  .win-controls {
    position: relative;
    z-index: 410;
    display: flex;
    align-self: stretch;
  }
  .win-btn {
    display: grid;
    place-items: center;
    width: var(--caption-btn-w);
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
     un-fill it on the way down. The heavier fill carries the press instead. */
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

  /* One floating panel on the window's own backdrop, rather than a region
     butted against the frame.

     The gutter is what makes the glass real. Cards cover most of the content
     plane and the plane covers the window, so with everything butted edge to
     edge the backdrop only ever showed through the sum of the layer alphas —
     technically translucent, visually a flat tint. This strip is unpainted
     window: honest, undiluted material that frames the app and cannot be
     covered by any card. It is also what tells the eye the plane is floating
     *on* something, which is the whole point of a material and the thing a
     uniform tint can never say. */
  .body {
    flex: 1 1 auto;
    display: flex;
    min-height: 0;
    padding: var(--panel-gap);
  }

  .content-clip {
    position: relative;
    flex: 1 1 auto;
    display: flex;
    min-width: 0;
    overflow: hidden;
    border-radius: var(--radius-lg);
    background: var(--material-content);
    box-shadow: inset 0 0 0 1px var(--border), var(--glass-sheen);
  }

  .content {
    flex: 1 1 auto;
    min-width: 0;
    /* Explicit, not inherited: `overflow-y: auto` alone computes overflow-x
       to `auto` as well, which is what made the whole screen draggable
       sideways on a touchpad even with nothing to scroll to. */
    overflow-x: hidden;
    overflow-y: auto;
    overscroll-behavior: none;
    /* Sections size themselves against the reading column, not the window.
       A viewport media query cannot see this — which is how a setting row with
       a four-option segmented picker ended up squeezed at exactly the size it
       was supposed to stack at. */
    container: content / inline-size;
    --scroll-edge: 0;
    /* How much room the last card has to leave for the dock floating over it.
       Owned here rather than in Section.svelte because it is a fact about this
       layout, not about a section: one dock item (--hit), plus the capsule's
       padding on both sides, plus how far it floats above the plane (Dock's
       `bottom`), plus a card's worth of air.
       Computed rather than the hand-summed `100px` it used to be: that number
       was tied to the dock item's size with nothing saying so, so resizing the
       dock would have left the last card permanently half covered — the same
       class of bug Section.svelte's own note records having fixed once. */
    --dock-clear: calc(
      var(--hit) + 2 * var(--space-1) + var(--space-4) + var(--space-8)
    );
  }
  .content.scrolled {
    --scroll-edge: 1;
  }
  /* The one legitimate focus-ring removal in the app. app.css's global rule
     matches `[tabindex]:focus-visible`, which now includes this container — but
     it is a scroll surface, not a control, and it is focused programmatically on
     every navigation. Ringing the whole content plane on each section change
     would read as an error state. Keyboard users lose nothing: the ring belongs
     on the controls inside, which still get it. */
  .content:focus,
  .content:focus-visible {
    outline: none;
  }
</style>
