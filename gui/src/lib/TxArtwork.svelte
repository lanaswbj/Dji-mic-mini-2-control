<script>
  import sprite from "../assets/devices/mic-mini-2-cover-cutouts.png";
  import { txCover, txCoverPosition } from "./txCovers.js";

  /**
   * The transmitter wearing a specific magnetic front cover, cut from one
   * sprite sheet.
   *
   * `size` defaults to what the only caller actually asks for (TxCard's 72). It
   * used to default to 124 — a number no call site passes and no layout
   * contains, so the component advertised a size it had never once been
   * rendered at.
   */
  let { value = "obsidian-black", size = 72 } = $props();
  const selected = $derived(txCover(value));
</script>

<span
  class="artwork"
  style:width={`${size}px`}
  style:height={`${size}px`}
  style:background-image={`url(${sprite})`}
  style:background-position={txCoverPosition(selected)}
  role="img"
  aria-label={`DJI Mic Mini 2，${selected.name}前盖`}
></span>

<style>
  .artwork {
    display: block;
    background-repeat: no-repeat;
    background-size: 500% 200%;
  }
</style>
