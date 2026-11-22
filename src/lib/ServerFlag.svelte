<script lang="ts">
  // Where a download's server is: its country's flag and name, the address, the round trip.
  import type { ServerView } from "$lib/api";

  let { server, compact = false }: { server: ServerView; compact?: boolean } = $props();

  let names: Intl.DisplayNames | null = null;
  try {
    names = new Intl.DisplayNames(["en"], { type: "region" });
  } catch {
    names = null;
  }
  const country = $derived(server.country ? (names?.of(server.country) ?? server.country) : server.proxied ? "Through a proxy" : "Local network");
  const title = $derived(
    [server.ip, server.cdn ? `${server.cdn} (a server near you)` : "", server.proxied ? "the proxy's address" : ""].filter(Boolean).join(" · "),
  );
</script>

<span class="sv" {title}>
  {#if server.flag}
    <i class="flag" style="background-position:-{server.flag[0] / 2}px -{server.flag[1] / 2}px" aria-hidden="true"></i>
  {/if}
  <span class="name">{country}</span>
  {#if !compact}<span class="mono meta">{server.ip}{server.rtt_ms != null ? ` · ${server.rtt_ms} ms` : ""}</span>{/if}
</span>

<style>
  .sv { display: inline-flex; align-items: center; gap: 6px; min-width: 0; }
  /* flags.png: 16 columns of 32×24 cells, shown at half size. */
  .flag { flex: none; width: 16px; height: 12px; border-radius: 2px; background: url("/flags.png") no-repeat; background-size: 256px auto; box-shadow: 0 0 0 1px var(--line2); }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta { color: var(--muted); font-size: 12px; white-space: nowrap; }
  .mono { font-family: var(--mono); font-variant-numeric: tabular-nums; }
</style>
