<script lang="ts">
  import { applyTheme } from "$lib/theme/apply";
  // A download's progress window: the bar, what's in and how fast, its parts as the
  // engine splits them, and Pause, Hide, Cancel; when it's done, Open and Open folder.
  import Icon from "$lib/Icon.svelte";
  import { api, inTauri, type Details, type Row } from "$lib/api";
  import type { IconName } from "$lib/icons";
  import * as fmt from "$lib/format";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onDestroy, onMount } from "svelte";
  import ErrorBox from "$lib/ErrorBox.svelte";
  import ServerFlag from "$lib/ServerFlag.svelte";

  let { id }: { id: number } = $props();

  let row = $state<Row | null>(null);
  let details = $state<Details | null>(null);
  let gone = $state("");
  let confirmCancel = $state(false);

  const ICONS: Record<string, IconName> = { video: "video", music: "music", docs: "doc", archives: "archive", programs: "app", images: "image", torrents: "magnet" };
  const pct = $derived(row ? (row.state === "done" ? 100 : fmt.percent(row.done, row.size)) : null);
  const file = $derived(details?.kind === "file" ? details.file : null);
  const parts = $derived(file?.segments ?? []);
  const total = $derived(parts.length ? parts[parts.length - 1][2] : 0);
  const statusText = $derived.by(() => {
    if (!row) return "";
    switch (row.state) {
      case "downloading": return row.size ? "Downloading" : "Downloading (size unknown)";
      case "queued": return "Waiting in the queue";
      case "paused": return "Paused";
      case "failed": return "Stopped";
      case "done": return "Download complete";
    }
  });

  async function load() {
    if (!inTauri) {
      row = demoRow();
      details = demoDetails();
      return;
    }
    try {
      row = await invoke<Row>("download_row", { id });
      if (row.state === "downloading" || details == null) details = await api.details(id).catch(() => details);
    } catch (e) {
      gone = String(e);
    }
  }

  // The window's title shows how far it is (seen on the taskbar).
  $effect(() => {
    if (!inTauri || !row) return;
    const title = row.state === "done" ? `Complete · ${row.display}` : `${pct != null ? `${Math.floor(pct)}% ` : ""}${row.display}`;
    getCurrentWindow().setTitle(title).catch(() => {});
  });

  const close = () => (inTauri ? getCurrentWindow().close() : undefined);
  const act = async (p: Promise<unknown>) => {
    try {
      await p;
    } catch (e) {
      gone = String(e);
    }
    load();
  };

  let timer: ReturnType<typeof setInterval>;
  onMount(() => {
    if (inTauri) invoke("watch_progress", { id }).catch(() => {});
    load();
    timer = setInterval(load, 500);
    if (inTauri) {
      api.settings().then((s) => {
        applyTheme(s.theme);
      });
    }
  });
  onDestroy(() => clearInterval(timer));

  // ---- sample data for the browser preview
  let demoDone = 812_000_000;
  function demoRow(): Row {
    demoDone = Math.min(2_097_152_000, demoDone + 9_000_000);
    return {
      id, url: "https://fuckingfast.co/tse0e990adkj#Hozy_--_fitgirl-repacks.site_--_.part1.rar", name: null, dir: "D:\\Downloads\\Compressed",
      file: "D:\\Downloads\\Compressed\\Hozy_--_fitgirl-repacks.site_--_.part1.rar", size: 2_097_152_000, done: demoDone,
      state: "downloading", error: null, category: "archives", referer: null, created_at: 0, finished_at: null, kind: "file",
      display: "Hozy_--_fitgirl-repacks.site_--_.part1.rar", speed: 18_400_000, eta: 70, conns: 8,
    } as Row;
  }
  function demoDetails(): Details {
    const size = 2_097_152_000;
    const n = 8;
    const fill = [1, 0.62, 0.55, 0.48, 0.41, 0.33, 0.27, 0.2];
    const segments: [number, number, number][] = Array.from({ length: n }, (_, i) => {
      const s = Math.floor((size / n) * i), e = Math.floor((size / n) * (i + 1));
      return [s, s + Math.floor((e - s) * fill[i]), e];
    });
    return { kind: "file", item: { limit_kib: 0, priority: 0, checksum: null, password: false }, file: { segments, connections: segments.map(() => [0, 2_300_000, true]), resumable: true, server: { ip: "88.198.54.17", country: "DE", flag: [256, 72], rtt_ms: 24, proxied: false, cdn: null } } };
  }
</script>

<div class="pw">
  {#if gone && !row}
    <div class="gone"><p>{gone}</p><button class="b" onclick={close}>Close</button></div>
  {:else if row}
    <div class="head">
      <span class="ficon" class:ok={row.state === "done"}><Icon name={row.state === "done" ? "done" : (ICONS[row.category] ?? "file")} size={20} /></span>
      <div class="what">
        <b title={row.display}>{row.display}</b>
        <span class="status" class:good={row.state === "downloading" || row.state === "done"} class:bad={row.state === "failed"}>{statusText}</span>
      </div>
    </div>

    <div class="big">
      <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={pct ?? undefined} aria-label="Progress">
        <i style="width:{pct ?? (row.state === 'downloading' ? 100 : 0)}%" class:indet={pct == null && row.state === "downloading"} class:paused={row.state === "paused" || row.state === "queued"} class:failed={row.state === "failed"} class:done={row.state === "done"}></i>
      </div>
      <span class="pc mono">{pct != null ? `${pct.toFixed(1)}%` : ""}</span>
    </div>

    <dl class="facts">
      <div><dt>File size</dt><dd class="mono">{fmt.bytes(row.size)}</dd></div>
      <div><dt>Downloaded</dt><dd class="mono">{fmt.bytes(row.state === "done" ? row.size : row.done)}</dd></div>
      <div><dt>Transfer rate</dt><dd class="mono">{row.state === "downloading" ? fmt.speed(row.speed) : "—"}</dd></div>
      <div><dt>Time left</dt><dd class="mono">{row.state === "downloading" ? fmt.eta(row.eta) || "estimating" : "—"}</dd></div>
      <div><dt>{(row.kind === "torrent" ? "Peers" : "Connections")}</dt><dd class="mono">{row.state === "downloading" ? row.conns : "—"}</dd></div>
      <div><dt>Resume</dt><dd>{file?.resumable == null ? "—" : file.resumable ? "Yes" : "No"}</dd></div>
      {#if file?.server && row.state === "downloading"}
        <div class="wide"><dt>{file.server.proxied ? "Proxy" : "Server"}</dt><dd><ServerFlag server={file.server} /></dd></div>
      {/if}
    </dl>

    {#if parts.length > 1 && row.state !== "done"}
      <div class="lb">Parts ({parts.length} connections)</div>
      <div class="segmap" aria-label="Parts downloaded">
        {#each parts as [s, c, e], i (i)}
          <span style="flex:{Math.max(1, e - s)}"><i style="width:{e > s ? ((c - s) / (e - s)) * 100 : 100}%"></i></span>
        {/each}
      </div>
      {#if total}<div class="scale mono"><span>0</span><span>{fmt.bytes(total)}</span></div>{/if}
    {/if}

    {#if row.state === "failed" && row.error}<div class="ebw"><ErrorBox row={row} onsay={(t) => (gone = t)} /></div>{/if}

    <div class="addr mono" title={row.url}>{row.url}</div>

    <div class="foot">
      {#if row.state === "done"}
        <button class="b" onclick={close}>Close</button>
        <span class="grow"></span>
        <button class="b" onclick={() => act(api.reveal(row!.id))}><Icon name="folder" size={15} />Open folder</button>
        <button class="b primary" onclick={() => act(api.open(row!.id).then(close))}><Icon name="open" size={15} />Open</button>
      {:else if confirmCancel}
        <span class="ask">Remove this download and delete what's downloaded?</span>
        <span class="grow"></span>
        <button class="b" onclick={() => (confirmCancel = false)}>Keep it</button>
        <button class="b danger" onclick={() => act(api.remove([row!.id], true).then(close))}>Remove</button>
      {:else}
        <button class="b" onclick={() => (confirmCancel = true)}>Cancel</button>
        <span class="grow"></span>
        <button class="b" onclick={close} title="Close this window; the download goes on">Hide</button>
        {#if row.state === "downloading" || row.state === "queued"}
          <button class="b primary" onclick={() => act(api.pause([row!.id]))}><Icon name="pause" size={15} />Pause</button>
        {:else}
          <button class="b primary" onclick={() => act(api.resume([row!.id]))}><Icon name={row.state === "failed" ? "retry" : "play"} size={15} />{row.state === "failed" ? "Retry" : "Resume"}</button>
        {/if}
      {/if}
    </div>
  {/if}
</div>

<style>
  :global(html, body) { overflow: hidden; }
  .pw { height: 100vh; display: flex; flex-direction: column; padding: 16px 20px 16px; background: var(--bg); color: var(--text); }
  .head { display: flex; align-items: center; gap: 12px; }
  .ficon { width: 42px; height: 42px; border-radius: 10px; border: 1px solid var(--line2); display: grid; place-items: center; color: var(--text-2); flex: none; background: var(--card); }
  .ficon.ok { color: var(--up); border-color: color-mix(in srgb, var(--up) 40%, transparent); }
  .what { min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .what b { font-size: 14.5px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .status { font-size: 12.5px; color: var(--muted); }
  .good { color: var(--up); }
  .bad { color: var(--down); }
  .big { display: flex; align-items: center; gap: 12px; margin: 16px 0 6px; }
  .bar { flex: 1; height: 12px; border-radius: 6px; background: var(--line2); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--bar); border-radius: 6px; transition: width 450ms linear; }
  .bar i.paused { background: var(--muted); }
  .bar i.failed { background: var(--down); }
  .bar i.indet { opacity: 0.45; animation: pulse 1.2s ease-in-out infinite; }
  .pc { width: 58px; text-align: right; font-size: 14px; font-weight: 600; }
  .mono { font-family: var(--mono); font-variant-numeric: tabular-nums; }
  .facts { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px 16px; margin: 10px 0 0; padding: 12px 14px; border: 1px solid var(--line); border-radius: 10px; background: var(--card); }
  .facts dt { font-size: 11px; color: var(--muted); letter-spacing: 0.03em; }
  .facts dd { margin: 2px 0 0; font-size: 13px; color: var(--text); }
  .facts .wide { grid-column: 1 / -1; }
  .lb { color: var(--muted); font-size: 11px; margin: 12px 0 5px; letter-spacing: 0.03em; }
  .segmap { display: flex; gap: 2px; height: 14px; }
  .segmap span { background: var(--line2); border-radius: 2px; overflow: hidden; min-width: 2px; }
  .segmap i { display: block; height: 100%; background: var(--bar); transition: width 450ms linear; }
  .scale { display: flex; justify-content: space-between; font-size: 10.5px; color: var(--muted); margin-top: 3px; }
  .ebw { margin-top: 10px; }
  .addr { margin-top: 10px; font-size: 11.5px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; user-select: text; }
  .foot { margin-top: auto; padding-top: 14px; display: flex; gap: 8px; align-items: center; border-top: 1px solid var(--line); }
  .grow { flex: 1; }
  .ask { font-size: 12.5px; color: var(--text-2); }
  .b { height: 36px; padding: 0 16px; border-radius: 8px; border: 1px solid var(--line2); background: transparent; color: var(--text-2); font-size: 13px; display: inline-flex; align-items: center; gap: 7px; }
  .b:hover { border-color: var(--line3); color: var(--text); }
  .b.primary { background: var(--hero); color: var(--hero-text); border-color: var(--hero); font-weight: 500; }
  .b.primary:hover { color: var(--hero-text); filter: brightness(0.92); }
  .b.danger { color: var(--down); border-color: color-mix(in srgb, var(--down) 40%, transparent); }
  .gone { margin: auto; display: flex; flex-direction: column; align-items: center; gap: 10px; color: var(--text-2); text-align: center; }
  @keyframes pulse { 50% { opacity: 0.15; } }
</style>
