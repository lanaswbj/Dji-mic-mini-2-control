<script>
  import Section from "./Section.svelte";
  import Card from "../ui/Card.svelte";
  import Button from "../ui/Button.svelte";
  import Icon from "../ui/Icon.svelte";
  import { pieOrder } from "../pieOrder.svelte.js";
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
   * constants the code that runs them is compiled against — so this page cannot
   * describe a menu the app doesn't actually have.
   *
   * **Reordering lives here, not on the overlay.** The overlay is a transient,
   * keyboard/tap-driven thing that appears above the taskbar for a second at a
   * time; putting a drag gesture on it would mean every arc press had to be
   * disambiguated from the start of a rearrangement, on the one control where a
   * misread press fires an action into whatever window you were just using.
   * Here the list is sitting still, the labels and the effects are readable,
   * and nothing is at stake if a drag is misjudged.
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

  /**
   * The slots in the user's order. Falls back to the backend's own order if the
   * two disagree about how many slots exist — a stored order from a build with
   * a different slot count must not be able to hide one.
   */
  const ordered = $derived(
    slots.length === pieOrder.indices.length ? pieOrder.indices.map((i) => slots[i]) : slots,
  );
  /** Everything but the pinned close slot at the end. */
  const movable = $derived(Math.max(0, ordered.length - 1));

  // --- Drag to reorder --------------------------------------------------
  // Rows are not the same height (an effect line wraps on a narrow window), so
  // nothing here may assume a fixed pitch: the geometry is measured once at
  // drag start and every offset is derived from those measurements.

  /** Pointer travel before a press turns into a drag. */
  const DRAG_THRESHOLD = 5;
  /** Must match the `transform` transition on `.row.settling`. */
  const SETTLE_MS = 240;

  let listEl = $state(null);
  let dragIndex = $state(null);
  let targetIndex = $state(null);
  let dy = $state(0);
  let settling = $state(false);
  /** `{ top, h }` per row, in the layout as it stood when the drag began. */
  let geom = [];
  let settleTimer = 0;

  function measure() {
    const rows = listEl?.querySelectorAll(".row");
    return rows ? [...rows].map((el) => ({ top: el.offsetTop, h: el.offsetHeight })) : [];
  }

  /** How far row `i` has been pushed aside to open a gap for the dragged one.
   *  Displaced rows all move by exactly the dragged row's height, whatever
   *  their own is — that is what makes the gap the right size. */
  function shiftFor(i) {
    if (dragIndex === null || i === dragIndex) return 0;
    const h = geom[dragIndex]?.h ?? 0;
    if (targetIndex > dragIndex && i > dragIndex && i <= targetIndex) return -h;
    if (targetIndex < dragIndex && i >= targetIndex && i < dragIndex) return h;
    return 0;
  }

  const offsetFor = (i) => (i === dragIndex ? dy : shiftFor(i));

  /** Where the dragged row's top ends up once the list is actually reordered.
   *  Moving down, it lands flush with the *bottom* of the row it passed. */
  function restingOffset(from, to) {
    if (!geom[from] || !geom[to]) return 0;
    if (to > from) return geom[to].top + geom[to].h - geom[from].h - geom[from].top;
    return geom[to].top - geom[from].top;
  }

  function onPointerDown(e, i) {
    if (e.button !== 0 || i >= movable || movable < 2) return;
    const startY = e.clientY;
    const el = e.currentTarget;
    let started = false;

    const move = (ev) => {
      const delta = ev.clientY - startY;
      if (!started) {
        if (Math.abs(delta) < DRAG_THRESHOLD) return;
        started = true;
        geom = measure();
        dragIndex = i;
        targetIndex = i;
        settling = false;
        el.setPointerCapture?.(ev.pointerId);
      }
      dy = delta;

      // Walk outward from the source slot, past any row whose own midpoint the
      // dragged row's midpoint has crossed. Comparing midpoints rather than
      // tops is what stops a tall row from being impossible to drag past a
      // short one.
      const centre = geom[i].top + geom[i].h / 2 + dy;
      let to = i;
      if (delta > 0) {
        while (to + 1 < movable && centre > geom[to + 1].top + geom[to + 1].h / 2) to++;
      } else {
        while (to - 1 >= 0 && centre < geom[to - 1].top + geom[to - 1].h / 2) to--;
      }
      targetIndex = to;
    };

    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      if (started) settle();
    };

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  }

  /**
   * Release. The dragged row eases the last few pixels into its slot, and the
   * list is only reordered once it is already sitting exactly there — so the
   * reorder, which happens instantly, moves nothing on screen. Committing on
   * `pointerup` instead would jump the row by up to half its height.
   */
  function settle() {
    if (dragIndex === null) return;
    settling = true;
    dy = restingOffset(dragIndex, targetIndex);
    clearTimeout(settleTimer);
    settleTimer = setTimeout(commit, SETTLE_MS);
  }

  function commit() {
    if (dragIndex === null) return;
    moveTo(dragIndex, targetIndex);
    dragIndex = null;
    targetIndex = null;
    dy = 0;
    settling = false;
  }

  /** The one place the order is actually written, so the drag and the keyboard
   *  path cannot drift apart. */
  function moveTo(from, to) {
    if (from === to) return;
    const next = [...pieOrder.indices];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    pieOrder.set(next);
  }

  /** Keyboard equivalent, so reordering isn't pointer-only. */
  function onKeydown(e, i) {
    if (!e.altKey || i >= movable) return;
    const to = e.key === "ArrowUp" ? i - 1 : e.key === "ArrowDown" ? i + 1 : i;
    if (to === i || to < 0 || to >= movable) return;
    e.preventDefault();
    moveTo(i, to);
    // The row moved out from under the focus ring; put it back on it.
    requestAnimationFrame(() => listEl?.querySelectorAll(".row")[to]?.focus());
  }

  $effect(() => () => clearTimeout(settleTimer));
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

  <Card
    title="菜单项"
    icon="pie"
    subtitle="拖动可以调换顺序。菜单打开时选中项停在第一个，所以越靠前的越省一步。"
  >
    {#snippet actions()}
      {#if pieOrder.customised}
        <Button variant="ghost" icon="rollback" onclick={() => pieOrder.reset()}>恢复默认</Button>
      {/if}
    {/snippet}

    <ol class="slots" bind:this={listEl} class:dragging={dragIndex !== null}>
      {#each ordered as slot, i (slot.index)}
        <li>
          <!-- A button, not a div with a separate handle: the whole row is the
               grab target, and it has to be reachable and reorderable from the
               keyboard (Alt + Up/Down) for the feature to exist at all without
               a pointer. `type="button"` because it is in no form and must
               never submit anything. -->
          <button
            type="button"
            class="row"
            class:pinned={i >= movable}
            class:lifted={i === dragIndex}
            class:settling={i === dragIndex && settling}
            style:transform="translateY({offsetFor(i)}px)"
            aria-label={`${slot.label}，第 ${i + 1} 项${
              i >= movable ? "，固定在最后" : "，按住拖动，或用 Alt 加上下方向键调整顺序"
            }`}
            onpointerdown={(e) => onPointerDown(e, i)}
            onkeydown={(e) => onKeydown(e, i)}
          >
            <span class="grip" aria-hidden="true">
              {#if i >= movable}
                <Icon name="lock" size="sm" />
              {:else}
                <span class="dots"></span>
              {/if}
            </span>
            <span class="mark"><Icon name={slot.icon} size="sm" /></span>
            <span class="text">
              <span class="label">{slot.label}</span>
              <span class="u-caption">{slot.effect}</span>
            </span>
            <span class="pos u-num" aria-hidden="true">{i + 1}</span>
          </button>
        </li>
      {/each}
    </ol>
    <!-- Outside the <ol>, and marked the way every other "this cannot work right
         now" line in the app is (InputGestures' .warn-line). It used to be an
         <li> inside the ordered list, which numbered the failure message "1." as
         though it were a menu slot, and styled it as ordinary caption text — so
         the one thing on screen saying the feature is unavailable read quieter
         than the sentence above it explaining how to use it. -->
    {#if ordered.length === 0}
      <p class="u-caption u-icon-line warn-line">
        <Icon name="alert" size="sm" /><span>读取菜单项失败——快捷菜单在当前平台上不可用。</span>
      </p>
    {/if}
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
    margin: 0;
    padding: 0;
    list-style: none;
  }

  /* `.u-icon-line` (app.css) supplies the layout — these lines wrap on a narrow
     window, and centring the glyph against a two-line paragraph parks it
     halfway down the text. Only the type scale is local. */
  .how {
    gap: var(--space-3);
  }
  .how li {
    font-size: var(--type-caption-size);
    color: var(--text-secondary);
  }

  /* Same treatment as InputGestures' unavailable line: layout from
     `.u-icon-line` (app.css), tone from --warn. */
  .warn-line {
    color: var(--warn);
    margin-top: var(--space-3);
  }

  /* Tight, because the rows are a *sequence* — the gap has to say "these are
     one ordered thing", and at --space-3 they read as separate cards that
     happen to be stacked. */
  .slots {
    gap: var(--space-05);
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-2);
    border: 1px solid transparent;
    border-radius: var(--radius-md);
    background: none;
    color: var(--text);
    text-align: left;
    cursor: grab;
    /* Without this a drag on a touchscreen or precision touchpad is claimed by
       the scroller before the first pointermove ever arrives. */
    touch-action: none;
    transition: transform var(--dur-base) var(--ease-out),
      background var(--dur-fast) var(--ease-out),
      border-color var(--dur-fast) var(--ease-out);
  }
  .row:hover {
    background: var(--surface-sunken);
  }
  .row.pinned {
    cursor: default;
    color: var(--text-secondary);
  }
  .row.pinned:hover {
    background: none;
  }

  /* The dragged row follows the pointer 1:1 and must not be smoothed, or it
     rubber-bands behind the finger — Apple's direct-manipulation rule. It gets
     its transition back only for the short settle into the target slot. */
  .row.lifted {
    position: relative;
    z-index: 2;
    transition: none;
    border-color: var(--border);
    background: var(--surface-raised);
    box-shadow: var(--elev-2);
    cursor: grabbing;
  }
  .row.settling {
    transition: transform var(--dur-base) var(--ease-out);
  }

  /* app.css's reduced-motion block sets `transition-duration` on `*`, and
     `transition-property` defaults to `all` — so it hands a 160ms transition to
     `.row.lifted`, which sets `transition: none` precisely so a dragged row
     tracks the pointer 1:1. Under that rule the row rubber-banded 160ms behind
     the finger for the whole drag, which is worse than the motion being
     suppressed.
     `.row.settling` is restated for a different reason: its duration is
     hand-synced with `SETTLE_MS` in the script above. Letting the global rule
     shorten it to 160ms left the row finished and frozen for 80ms before
     `commit()` fired, on every drop. Pinning it keeps the two halves agreeing. */
  @media (prefers-reduced-motion: reduce) {
    .row.lifted {
      transition: none !important;
    }
    .row.settling {
      transition-duration: var(--dur-base) !important;
    }
  }
  /* Nothing may hover-highlight while a drag is in flight: the pointer is over
     whatever row it is passing, and lighting those up reads as the drag
     landing there. */
  .slots.dragging .row:not(.lifted):hover {
    background: none;
  }

  /* Two columns of dots — the universal "this is draggable" mark, drawn with a
     gradient rather than six elements or an icon, since it is pure texture. */
  .grip {
    display: grid;
    place-items: center;
    width: 16px;
    height: 24px;
    flex: 0 0 auto;
    color: var(--text-tertiary);
  }
  .dots {
    width: 6px;
    height: 14px;
    background-image: radial-gradient(currentColor 1px, transparent 1.2px);
    background-size: 4px 4px;
    opacity: 0.75;
  }

  .mark {
    display: grid;
    place-items: center;
    width: var(--glyph-md);
    height: var(--glyph-md);
    flex: 0 0 auto;
    border-radius: var(--radius-sm);
    background: var(--accent-soft);
    color: var(--accent);
  }
  .row.pinned .mark {
    background: var(--surface-sunken);
    color: var(--text-tertiary);
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: var(--space-05);
    min-width: 0;
    flex: 1 1 auto;
  }

  .label {
    font-size: var(--type-body-size);
    font-weight: 550;
  }

  /* The position number is the only thing that survives a drag as a fact rather
     than as a feeling — it is what the list is *for*. */
  .pos {
    flex: 0 0 auto;
    width: 20px;
    text-align: right;
    font-size: var(--type-label-size);
    font-weight: 600;
    color: var(--text-tertiary);
  }
</style>
