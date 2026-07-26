<script>
  import Section from "./Section.svelte";
  import Card from "../ui/Card.svelte";
  import EmptyState from "../ui/EmptyState.svelte";
  import SettingRow from "../SettingRow.svelte";
  import AccessIssueCard from "../AccessIssueCard.svelte";
  import { devices as store } from "../store.svelte.js";
  import { SETTING_HELP } from "../copy.js";

  /**
   * One protocol setting group, rendered as a single list.
   *
   * The old build offered two layouts for this — a flat list and a JS-masonry
   * "compact" grid whose column balancing used a made-up height estimate —
   * and a toggle between them. Two layouts for one list is a decision pushed
   * onto the user in place of making one; there is one layout now, and it's
   * the readable one.
   *
   * The three noise-cancel settings are the only ones with a real hierarchy
   * (mode and the transmitter button are meaningless without the power
   * toggle), so those two nest under it. That's structure the data already
   * describes, not a special case invented for looks.
   */
  let { group, icon = null } = $props();

  const NESTED_UNDER = { "noise-cancel": "noise-cancel-power", "noise-cancel-button": "noise-cancel-power" };
  // Settings addressed at one transmitter appear on that transmitter's own
  // card in 概览, not in this shared list where they'd imply a single value.
  const TX_TARGETED = new Set(["voice-tone"]);

  const items = $derived(store.settings.filter((s) => s.group === group));
  const rows = $derived(
    items
      .filter((s) => !TX_TARGETED.has(s.id) && !(s.id in NESTED_UNDER))
      .map((s) => ({
        setting: s,
        children: items.filter((c) => NESTED_UNDER[c.id] === s.id),
      })),
  );
</script>

<!-- The parent row and its nested children are the same row wired to the same
     store, so the wiring is written once here rather than twice below. -->
{#snippet settingRow(setting)}
  <SettingRow
    {setting}
    value={store.values[setting.id] ?? null}
    state={store.writeState(setting.id)}
    lockReason={store.lockReason(setting)}
    description={SETTING_HELP[setting.id] ?? null}
    onchange={(id, value) => store.change(id, value)}
  />
{/snippet}

<Section title={group} {icon} subtitle={store.device?.model_name ?? "未连接设备"}>
  {#if store.accessIssue}
    <AccessIssueCard />
  {:else if rows.length === 0}
    <Card>
      <EmptyState
        icon="plug"
        title="没有可调整的项目"
        description="连接受支持的麦克风后，它支持的设置会出现在这里。"
      />
    </Card>
  {:else}
    <Card>
      {#each rows as row (row.setting.id)}
        <div class="entry">
          {@render settingRow(row.setting)}
          {#if row.children.length > 0}
            <div class="nested">
              {#each row.children as child (child.id)}
                {@render settingRow(child)}
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </Card>
  {/if}
</Section>

<style>
  .entry {
    display: flex;
    flex-direction: column;
  }
  .entry + .entry {
    border-top: 1px solid var(--border);
  }

  /* The rule marks the dependency: these belong to the row above them. */
  .nested {
    display: flex;
    flex-direction: column;
    margin-left: var(--space-4);
    padding-left: var(--space-4);
    border-left: 2px solid var(--border);
  }
</style>
