<script module>
  /*
   * The app's only icon source. One family, one grid (24x24), one stroke
   * weight — mixing icon sets (or using emoji, as the old 🔒 / ⚠ labels did)
   * is what makes a UI read as assembled rather than designed.
   *
   * Paths are hand-authored geometry rather than lifted from an icon library,
   * so there is no third-party license to carry and no build step — the same
   * reasoning the rest of this codebase applies to dependencies.
   *
   * Every path is stroked with `currentColor`, so an icon always inherits the
   * color of whatever it sits in and needs no per-usage theming.
   */
  export const ICONS = {
    // --- Navigation ---------------------------------------------------
    gauge: "M4 16.5a8 8 0 1 1 16 0M12 16.5l4.5-5",
    audio: "M4 10v4M8 6.5v11M12 9v6M16 4.5v15M20 10v4",
    power: "M12 3.5v8.5M7 6.6a7 7 0 1 0 10 0",
    chip: "M7.5 7.5h9v9h-9zM10 3.5v4M14 3.5v4M10 16.5v4M14 16.5v4M3.5 10h4M3.5 14h4M16.5 10h4M16.5 14h4",
    tap: "M12 12h.01M9 9a4.2 4.2 0 0 0 0 6M15 9a4.2 4.2 0 0 1 0 6M6.2 6.2a8.2 8.2 0 0 0 0 11.6M17.8 6.2a8.2 8.2 0 0 1 0 11.6",
    pie: "M3.5 17.5a8.5 8.5 0 0 1 17 0M7 11.4h.01M12 9h.01M17 11.4h.01",
    sliders: "M4 7.5h10M19.5 7.5h.5M4 16.5h4.5M14 16.5h6M17 7.5a1.6 1.6 0 1 1-3.2 0 1.6 1.6 0 0 1 3.2 0M11.7 16.5a1.6 1.6 0 1 1-3.2 0 1.6 1.6 0 0 1 3.2 0",

    // --- Status / feedback --------------------------------------------
    check: "M5 12.5l4.5 4.5L19 7.5",
    x: "M6.5 6.5l11 11M17.5 6.5l-11 11",
    alert: "M12 4.5l8.5 15h-17zM12 10v4.2M12 17h.01",
    info: "M12 20.5a8.5 8.5 0 1 0 0-17 8.5 8.5 0 0 0 0 17M12 11.5V16M12 8h.01",
    lock: "M6.8 11h10.4v8.5H6.8zM9.4 11V8.2a2.6 2.6 0 0 1 5.2 0V11",
    question:
      "M12 20.5a8.5 8.5 0 1 0 0-17 8.5 8.5 0 0 0 0 17M9.7 9.6A2.4 2.4 0 0 1 14.4 10.2c0 1.6-2.4 2-2.4 3.4M12 17h.01",
    target:
      "M12 20.5a8.5 8.5 0 1 0 0-17 8.5 8.5 0 0 0 0 17M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8M12 12h.01",
    shield: "M12 3.6l7 2.6v5.3c0 4.2-2.9 7.2-7 8.9-4.1-1.7-7-4.7-7-8.9V6.2z",
    clock: "M12 20.5a8.5 8.5 0 1 0 0-17 8.5 8.5 0 0 0 0 17M12 7.4V12l3.1 1.9",

    // --- Chrome / actions ---------------------------------------------
    "chevron-down": "M6.5 9.5l5.5 5.5 5.5-5.5",
    "chevron-up": "M6.5 14.5l5.5-5.5 5.5 5.5",
    "chevron-right": "M9.5 5.5l6.5 6.5-6.5 6.5",
    "corner-down-left": "M19 5v7.5a2 2 0 0 1-2 2H6M9.5 11L6 14.5 9.5 18",
    refresh: "M20 12a8 8 0 1 1-2.5-5.8M20 4v4.5h-4.5",
    rollback: "M4 12a8 8 0 1 0 2.5-5.8M4 4v4.5h4.5",
    sidebar: "M4 5h16v14H4zM10 5v14",
    minimize: "M5 12h14",
    maximize: "M5.5 5.5h13v13h-13z",
    restore: "M8.5 8.5h10v10h-10zM5.5 15.5v-10h10",
    download: "M12 4v11M7.5 10.5l4.5 4.5 4.5-4.5M5 19.5h14",
    copy: "M9 9h10.5v10.5H9zM15 9V4.5H4.5V15H9",
    trash: "M4.5 7h15M9.5 7V4.5h5V7M6.5 7l1 12.5h9L17.5 7",
    plus: "M12 5.5v13M5.5 12h13",
    window: "M3.5 5h17v14h-17zM3.5 9.2h17M6.4 7.1h.01M9 7.1h.01",
    tray: "M4.5 13.8h4.4l1.4 2.4h3.4l1.4-2.4h4.4M6.2 5h11.6l2 8.8v5.2h-15.6v-5.2z",

    // --- Domain --------------------------------------------------------
    mic: "M12 4a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0V7a3 3 0 0 1 3-3M6.5 11a5.5 5.5 0 0 0 11 0M12 16.8v3.2M9 20h6",
    bolt: "M13.2 3L5.5 14h5.6l-.9 7 7.8-11h-5.6z",
    keyboard: "M3.5 6.5h17v11h-17zM7 10h.01M11 10h.01M15 10h.01M17.5 10h.01M6 13.8h.01M8.5 14h7",
    plug: "M9 3.5v5.5M15 3.5v5.5M6.8 9h10.4v2.6a5.2 5.2 0 0 1-10.4 0zM12 16.8v3.7",
    box: "M12 3.2l8 4.4v8.8l-8 4.4-8-4.4V7.6zM4 7.6l8 4.4 8-4.4M12 12v8.8",
    layers: "M12 3.4l8.4 4.3-8.4 4.3-8.4-4.3zM3.6 12.2l8.4 4.3 8.4-4.3M3.6 16.4l8.4 4.3 8.4-4.3",
    palette:
      "M12 20.5a8.5 8.5 0 1 1 0-17c4.7 0 8.5 3.2 8.5 7.1 0 2.3-2 3.4-3.6 3.4h-1.7a2 2 0 0 0-1.3 3.5c.5.6.2 3-1.9 3M8 9h.01M12 7.5h.01M16 9.5h.01",
    sparkle: "M12 3.5l1.9 5.6 5.6 1.9-5.6 1.9L12 18.5l-1.9-5.6L4.5 11l5.6-1.9z",
    /* One sine period, and the same period struck through — the noise-cancel
       pair. Drawn as one wave rather than the usual "speaker with arcs" so it
       reads at 16px, where three concentric arcs turn to mud. */
    wave: "M3.5 12c1.4-4.2 2.9-4.2 4.3 0s2.9 4.2 4.3 0 2.9-4.2 4.3 0 2.9 4.2 4.1 0",
    "wave-off":
      "M3.5 12c1.4-4.2 2.9-4.2 4.3 0s2.9 4.2 4.3 0 2.9-4.2 4.3 0 2.9 4.2 4.1 0M5 19.5l14-15",
    filter: "M3.8 5.5h16.4l-6.4 7.4v6l-3.6 2v-8z",
    stereo:
      "M9.4 15.6a3.9 3.9 0 1 1 0-7.8 3.9 3.9 0 0 1 0 7.8M14.6 15.6a3.9 3.9 0 1 1 0-7.8 3.9 3.9 0 0 1 0 7.8",
    camera:
      "M4 8.2h3.6L9.1 5.6h5.8l1.5 2.6H20v10.3H4zM15.2 13.3a3.2 3.2 0 1 1-6.4 0 3.2 3.2 0 0 1 6.4 0",
    bulb: "M9.6 18.2h4.8M10.4 21h3.2M12 3.2a5.9 5.9 0 0 1 3.5 10.7c-.7.5-1.1 1.3-1.1 2.2H9.6c0-.9-.4-1.7-1.1-2.2A5.9 5.9 0 0 1 12 3.2",
    tag: "M4.2 4.2h6.6l9 9-6.6 6.6-9-9zM7.6 7.6h.01",
    hash: "M9.8 4.4L8.2 19.6M15.8 4.4l-1.6 15.2M4.4 9.4h15.2M3.9 14.6h15.2",
    link: "M10.4 13.6a4.1 4.1 0 0 0 5.8 0l2.3-2.3a4.1 4.1 0 1 0-5.8-5.8l-1.3 1.3M13.6 10.4a4.1 4.1 0 0 0-5.8 0l-2.3 2.3a4.1 4.1 0 1 0 5.8 5.8l1.3-1.3",

    // --- Theme ---------------------------------------------------------
    sun: "M12 15.8a3.8 3.8 0 1 0 0-7.6 3.8 3.8 0 0 0 0 7.6M12 3v2.2M12 18.8V21M3 12h2.2M18.8 12H21M5.6 5.6l1.6 1.6M16.8 16.8l1.6 1.6M18.4 5.6l-1.6 1.6M7.2 16.8l-1.6 1.6",
    moon: "M20 14.3A8.6 8.6 0 0 1 9.7 4 8.5 8.5 0 1 0 20 14.3",
    monitor: "M3.5 5h17v11h-17zM9 20h6M12 16v4",
  };
</script>

<script>
  /**
   * `name` must be a key of ICONS. `size` is one of the three icon-size
   * tokens — a fourth size is a design-system violation, not a prop, and for
   * the same reason the 1.6 stroke is fixed in the markup rather than
   * overridable: one family means one weight.
   *
   * Decorative by default (`aria-hidden`). Pass `label` only when the icon is
   * the *sole* carrier of meaning; then it becomes an `img` role with a name.
   */
  let { name, size = "md", label = null } = $props();

  const SIZES = { sm: "var(--icon-sm)", md: "var(--icon-md)", lg: "var(--icon-lg)" };
  const d = $derived(ICONS[name]);
  const px = $derived(SIZES[size] ?? SIZES.md);
</script>

{#if d}
  <svg
    class="icon"
    style:width={px}
    style:height={px}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.6"
    stroke-linecap="round"
    stroke-linejoin="round"
    role={label ? "img" : "presentation"}
    aria-hidden={label ? undefined : "true"}
    aria-label={label}
  >
    <path {d} />
  </svg>
{/if}

<style>
  /* `inline-block`, not `block`. A block-level SVG dropped into an inline
     context (`<span>某某设置<Icon …/></span>`) forces its own line — that is
     exactly what pushed the chevron off the end of 概览's「全部音频设置」
     button onto a second row. Flex and grid blockify their children anyway, so
     every existing layout is unaffected and `vertical-align` is ignored there;
     the only behaviour that changes is the inline case, which was broken. */
  .icon {
    display: inline-block;
    vertical-align: middle;
    flex: 0 0 auto;
  }
</style>
