<script>
  /** Indeterminate progress. Reduced-motion turns the spin into a pulse. */
  let { size = "sm", label = "处理中" } = $props();
  const SIZES = { sm: "var(--icon-sm)", md: "var(--icon-md)", lg: "var(--icon-lg)" };
</script>

<span
  class="spinner"
  style:width={SIZES[size] ?? SIZES.sm}
  style:height={SIZES[size] ?? SIZES.sm}
  role="status"
  aria-label={label}
></span>

<style>
  /* A spinning ring is exactly the kind of continuous rotation reduced-motion
     is meant to stop — and it already is: app.css's global reduced-motion
     rule pins every animation in the app to one 1ms iteration with
     `!important`, which no local override here could outrank. The ring simply
     stops turning and the accent segment stays as the "still working" mark,
     so there is deliberately no second animation defined for that case. */
  .spinner {
    display: block;
    flex: 0 0 auto;
    border: 2px solid var(--border-strong);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin var(--dur-spin) linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(1turn);
    }
  }
</style>
