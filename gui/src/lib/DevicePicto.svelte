<script>
  // Resolve device artwork you drop into src/assets/devices/.
  // A file named "<pictogram-key>.png" (or .svg/.webp) is matched by key —
  // e.g. the DJI Mic Mini's key is "mic-mini" -> mic-mini.png.
  const files = import.meta.glob("../assets/devices/*.{png,svg,webp}", {
    eager: true,
    query: "?url",
    import: "default",
  });

  const art = {};
  for (const [path, url] of Object.entries(files)) {
    const stem = path.split("/").pop().replace(/\.(png|svg|webp)$/, "");
    art[stem] = url;
  }

  let { pictogram = "", size = 44 } = $props();
  const src = $derived(art[pictogram]);
</script>

{#if src}
  <img class="picto" {src} alt="" width={size} height={size} />
{:else}
  <!-- Neutral fallback so a new model renders before its artwork is supplied. -->
  <svg
    class="picto"
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    aria-hidden="true"
  >
    <rect x="8.5" y="2.5" width="7" height="12" rx="3.5" stroke="currentColor" stroke-width="1.5" />
    <path
      d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21M8.5 21h7"
      stroke="currentColor"
      stroke-width="1.5"
      stroke-linecap="round"
    />
  </svg>
{/if}

<style>
  .picto {
    display: block;
    object-fit: contain;
    color: inherit;
  }
</style>
