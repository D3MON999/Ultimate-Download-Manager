<script lang="ts">
  // The app's mark (design/icon.svg, "Cascade"): three chevrons falling and brightening.
  let { size = 22 }: { size?: number } = $props();
  const id = `lg${Math.random().toString(36).slice(2, 8)}`;
</script>

<svg width={size} height={size} viewBox="0 0 1024 1024" aria-hidden="true">
  <defs>
    <linearGradient id="{id}b" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#1d1e23" /><stop offset="1" stop-color="#09090b" /></linearGradient>
  </defs>
  <rect x="48" y="48" width="928" height="928" rx="216" fill="url(#{id}b)" stroke="#ffffff" stroke-opacity=".18" stroke-width="10" />
  <g fill="none" stroke="#ffffff" stroke-width="96" stroke-linecap="round" stroke-linejoin="round">
    <path d="M300 230 L512 400 L724 230" stroke-opacity=".3" />
    <path d="M300 410 L512 580 L724 410" stroke-opacity=".62" />
    <path d="M300 590 L512 760 L724 590" />
  </g>
</svg>
