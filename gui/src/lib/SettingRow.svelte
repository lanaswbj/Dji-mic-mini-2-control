<script>
  import Row from "./ui/Row.svelte";
  import Switch from "./ui/Switch.svelte";
  import Segmented from "./ui/Segmented.svelte";

  /**
   * One protocol `Setting` descriptor, rendered.
   *
   * This is the only place that knows how a descriptor's `kind` maps to a
   * control. The old build had that mapping copied into three components
   * (`SettingControl`, `NoiseCancelControl`, `TxFlipControl`), each with its
   * own switch markup, and they had already drifted apart.
   *
   * `"mixed"` — the two transmitters disagree — is shown as a mixed switch
   * *and* spelled out in words underneath, because a color and a knob
   * position are not an explanation.
   */
  /**
   * The subject glyph for each setting the protocol layer declares today.
   *
   * Keyed by `Setting.id` rather than passed in by every call site, because
   * the id is the one thing about a setting that is stable across the three
   * screens that render it (概览's quick block, the group list, and a
   * transmitter card) — a prop would have to be restated identically in all
   * three and would quietly drift. An id with no entry falls back to the
   * generic settings glyph rather than leaving a hole in the column, so a new
   * model declaring a new setting still renders correctly with nothing added
   * here.
   *
   * The two auto-off timers share `clock` on purpose: an icon names a *kind*
   * of setting, and their labels already say which device each one governs.
   */
  const ICONS = {
    "noise-cancel-power": "wave-off",
    "noise-cancel": "wave",
    "noise-cancel-button": "power",
    "low-cut": "filter",
    stereo: "stereo",
    "safety-track": "shield",
    "clip-limiter": "bolt",
    "auto-off-15m": "clock",
    "tx-auto-off-15m": "clock",
    "camera-power": "camera",
    "plug-free": "plug",
    "mic-leds": "bulb",
    "voice-tone": "sliders",
  };

  let {
    setting,
    value = null,
    state = "idle",
    lockReason = null,
    description = null,
    onchange,
  } = $props();

  const mixed = $derived(value === "mixed");
  const locked = $derived(!!lockReason);
  const unknown = $derived(value === null || value === undefined);
  const checked = $derived(mixed ? "mixed" : value === setting.options[1]?.value);

  const hint = $derived(
    mixed ? "两个发射器当前状态不同，点击后将统一开启" : description,
  );
</script>

<Row
  label={setting.label}
  icon={ICONS[setting.id] ?? "sliders"}
  description={hint}
  note={setting.note}
  {lockReason}
  {state}
>
  {#snippet control(descId)}
    {#if unknown}
      <span class="unknown" title="设备尚未上报这一项">—</span>
    {:else if setting.kind === "toggle"}
      <Switch
        {checked}
        disabled={locked || state === "writing"}
        label={setting.label}
        describedBy={descId}
        onchange={(on) =>
          onchange?.(setting.id, setting.options[on ? 1 : 0].value)}
      />
    {:else}
      <Segmented
        options={setting.options}
        value={mixed ? null : value}
        {mixed}
        disabled={locked || state === "writing"}
        label={setting.label}
        onchange={(v) => onchange?.(setting.id, v)}
      />
    {/if}
  {/snippet}
</Row>

<style>
  .unknown {
    padding: 0 var(--space-3);
    color: var(--text-tertiary);
    font-variant-numeric: tabular-nums;
  }
</style>
