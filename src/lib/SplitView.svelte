<script lang="ts">
  // The main window's list-and-detail layout: compact download cards on the left, the chosen
  // download in full on the right (the progress window's view, then its tabs). The table layout
  // stays as it was; Settings → App or the toolbar's layout button switches between them.
  import Icon from "$lib/Icon.svelte";
  import DetailsTabs from "$lib/DetailsTabs.svelte";
  import ErrorBox from "$lib/ErrorBox.svelte";
  import { friendly } from "$lib/errors";
  import { api, inTauri, type Details, type Row } from "$lib/api";
  import type { IconName } from "$lib/icons";
  import * as fmt from "$lib/format";

  type Acts = {
    run: (p: Promise<unknown>) => void;
    say: (text: string, bad?: boolean) => void;
    toggle: (r: Row) => void;
    remove: (ids: number[]) => void;
    add: () => void;
    extension: () => void;
  };

  let {
    rows,
    all,
    selected,
    focused,
    onselect,
    oncontext,
    needsCheck,
    needsYou,
    acts,
    filtering,
    dock = false,
  }: {
    rows: Row[];
    all: Row[];
    selected: Set<number>;
    focused: Row | null;
    onselect: (r: Row, e: MouseEvent) => void;
    oncontext: (r: Row, e: MouseEvent) => void;
    needsCheck: (r: Row) => boolean;
    needsYou: (r: Row) => boolean;
    acts: Acts;
    filtering: boolean;
    dock?: boolean;
  } = $props();

  const ICONS: Record<string, IconName> = { video: "video", music: "music", docs: "doc", archives: "archive", programs: "app", images: "image", torrents: "magnet" };
  const STATUS: Record<string, string> = { downloading: "Downloading", queued: "Queued", paused: "Paused", failed: "Failed", done: "Finished" };
  const statusOf = (r: Row) => (needsYou(r) ? "Needs you" : needsCheck(r) ? "Site check" : STATUS[r.state]);

  const active = $derived(all.filter((r) => r.state === "downloading"));
  const speed = $derived(active.reduce((a, r) => a + r.speed, 0));
  const queued = $derived(all.filter((r) => r.state === "queued").length);
  const doneToday = $derived.by(() => {
    const day = Math.floor(Date.now() / 1000) - 86_400;
    return all.filter((r) => r.state === "done" && (r.finished_at ?? 0) > day);
  });
  const many = $derived(rows.filter((r) => selected.has(r.id)));

  // The last two minutes of total speed, sampled each second (the overview's graph).
  let samples = $state<number[]>([]);
  $effect(() => {
    const timer = setInterval(() => {
      const now = all.filter((r) => r.state === "downloading").reduce((a, r) => a + r.speed, 0);
      samples = [...samples.slice(-119), now];
    }, 1000);
    return () => clearInterval(timer);
  });
  const peak = $derived(Math.max(1, ...samples));
  const graph = $derived(
    samples.length > 1
      ? samples.map((v, i) => `${((i / 119) * 100).toFixed(2)},${(40 - (v / peak) * 38).toFixed(2)}`).join(" ")
      : "",
  );

  // ---- Dock theme: the manifest strip (each part's progress) under arriving downloads, and the
  // chosen download's pieces filling in. Real part data from the engine; the demo makes some up.
  const STATUS_DOCK: Record<string, string> = { downloading: "Arriving", queued: "Up next", paused: "Stopped", failed: "Failed", done: "Delivered" };
  const statusText = (r: Row) => (dock && !needsYou(r) && !needsCheck(r) ? STATUS_DOCK[r.state] : statusOf(r));
  const CELLS = 512;

  /** Per-part fractions (0..1): a file's parts, or a torrent's pieces in 8 groups. */
  function fractions(d: Details): number[] {
    if (d.kind === "file") return (d.file?.segments ?? []).map(([s, c, e]) => (e > s ? Math.min(1, Math.max(0, (c - s) / (e - s))) : 1));
    const p = d.torrent.pieces;
    if (!p.length) return [];
    const n = Math.min(8, p.length);
    return Array.from({ length: n }, (_, i) => {
      const a = Math.floor((i * p.length) / n), b = Math.floor(((i + 1) * p.length) / n);
      let sum = 0;
      for (let k = a; k < b; k++) sum += p[k];
      return sum / Math.max(1, b - a);
    });
  }
  /** The file as CELLS cells: 1 arrived, 0.5 being fetched (a part's leading edge), 0 not yet. */
  function cells(d: Details): number[] {
    if (d.kind === "torrent") {
      const p = d.torrent.pieces;
      if (!p.length) return [];
      return Array.from({ length: CELLS }, (_, i) => {
        const a = Math.floor((i * p.length) / CELLS), b = Math.max(a + 1, Math.floor(((i + 1) * p.length) / CELLS));
        let sum = 0;
        for (let k = a; k < b; k++) sum += p[k] ?? 0;
        const v = sum / (b - a);
        return v >= 0.999 ? 1 : v > 0 ? 0.5 : 0;
      });
    }
    const segs = d.file?.segments ?? [];
    const total = segs.length ? segs[segs.length - 1][2] : 0;
    if (!total) return [];
    const out = new Array(CELLS).fill(0);
    for (const [s, c, e] of segs) {
      for (let i = Math.floor((s / total) * CELLS); i < Math.min(CELLS, Math.ceil((e / total) * CELLS)); i++) {
        const cs = (i / CELLS) * total, ce = ((i + 1) / CELLS) * total;
        if (c >= ce) out[i] = 1;
        else if (c > cs && c < e) out[i] = 0.5;
      }
    }
    return out;
  }
  function demoDetails(r: Row): Details {
    const pct = (fmt.percent(r.done, r.size) ?? 0) / 100;
    const size = r.size ?? 1_000_000_000, n = 8, part = size / n;
    const segments: [number, number, number][] = Array.from({ length: n }, (_, i) => {
      const skew = Math.min(1, Math.max(0, pct * (1.6 - i * 0.15) + (i === 0 ? 0.3 : 0)));
      return [i * part, i * part + part * skew, (i + 1) * part];
    });
    return { kind: "file", file: { segments, connections: [], resumable: true }, item: { limit_kib: 0, priority: 0, checksum: null, password: false } };
  }
  const load = (r: Row): Promise<Details> => (inTauri ? api.details(r.id) : Promise.resolve(demoDetails(r)));

  let strips = $state<Record<number, number[]>>({});
  $effect(() => {
    if (!dock) return;
    const tick = () => {
      const arriving = rows.filter((r) => r.state === "downloading").slice(0, 30);
      for (const r of arriving) load(r).then((d) => (strips = { ...strips, [r.id]: fractions(d) })).catch(() => {});
    };
    tick();
    const timer = setInterval(tick, 2000);
    return () => clearInterval(timer);
  });

  // The chosen download's cells, and when each one arrived (the last minute's light up in amber).
  let grid = $state<number[]>([]);
  let gridFor = -1;
  let arrivedAt: number[] = [];
  const focusedId = $derived(focused && focused.state !== "done" ? focused.id : -1);
  $effect(() => {
    const id = focusedId;
    if (!dock || id < 0) { grid = []; gridFor = -1; return; }
    const tick = () => {
      const r = all.find((x) => x.id === id);
      if (!r) return;
      load(r).then((d) => {
        if (focusedId !== id) return;
        const now = Date.now(), next = cells(d);
        if (gridFor !== id) { arrivedAt = next.map((v) => (v === 1 ? 0 : -1)); gridFor = id; }
        next.forEach((v, i) => { if (v === 1 && (arrivedAt[i] ?? -1) < 0) arrivedAt[i] = now; });
        grid = next.map((v, i) => (v === 1 && arrivedAt[i] > 0 && now - arrivedAt[i] < 60_000 ? 2 : v));
      }).catch(() => {});
    };
    tick();
    const timer = setInterval(tick, 1000);
    return () => clearInterval(timer);
  });
  const upNext = $derived(all.filter((r) => r.state === "queued").slice(0, 4));
  const toCome = $derived(all.filter((r) => r.state === "downloading" || r.state === "queued").reduce((a, r) => a + Math.max(0, (r.size ?? 0) - r.done), 0));

  function line(r: Row): string {
    const pct = fmt.percent(r.done, r.size);
    switch (r.state) {
      case "downloading":
        return [pct != null ? `${pct.toFixed(0)}%` : fmt.bytes(r.done), fmt.speed(r.speed), r.eta != null ? `${fmt.eta(r.eta)} left` : ""].filter(Boolean).join(" · ");
      case "done":
        return `${fmt.bytes(r.size)} · ${fmt.when(r.finished_at)}`;
      case "failed":
        return friendly(r.error).title;
      default:
        return `${pct != null ? `${pct.toFixed(0)}% · ` : ""}${fmt.bytes(r.done)} of ${fmt.bytes(r.size)}`;
    }
  }
</script>

<div class="split">
  <div class="list" role="listbox" aria-label="Downloads" aria-multiselectable="true">
    {#each rows as r (r.id)}
      {@const pct = fmt.percent(r.done, r.size)}
      <div
        id="row-{r.id}"
        class="card {r.state}"
        class:sel={selected.has(r.id)}
        role="option"
        aria-selected={selected.has(r.id)}
        tabindex="-1"
        onclick={(e) => onselect(r, e)}
        oncontextmenu={(e) => oncontext(r, e)}
        onkeydown={() => {}}
        ondblclick={() => (r.state === "done" ? acts.run(api.open(r.id)) : acts.run(api.showProgress(r.id)))}>
        <span class="ic"><Icon name={ICONS[r.category] ?? "file"} size={17} /></span>
        <div class="body">
          <div class="l1"><b title={r.display}>{r.display}</b><span class="st {r.state}" class:you={needsYou(r) || needsCheck(r)}>{statusText(r)}</span></div>
          {#if dock && r.state === "downloading" && strips[r.id]?.length}
            <div class="strip" aria-hidden="true">{#each strips[r.id] as f, i (i)}<i style="--f:{(f * 100).toFixed(1)}%"></i>{/each}</div>
          {:else if r.state !== "done"}
            <div class="pbar"><i style="width:{pct ?? (r.state === 'downloading' ? 100 : 0)}%" class:indet={pct == null && r.state === "downloading"}></i></div>
          {/if}
          <div class="l2 mono" title={line(r)}>{line(r)}</div>
          {#if r.tags}<div class="tags">{#each r.tags.split(" ") as tg (tg)}<span>#{tg}</span>{/each}</div>{/if}
        </div>
      </div>
    {:else}
      <div class="none">
        {#if filtering}
          <b>Nothing matches</b><span>Clear the filter or pick another category</span>
        {:else}
          <b>No downloads here</b><span>Add a link, or click a download in your browser</span>
          <button class="btn primary" onclick={acts.add}><Icon name="plus" size={16} />Add a link</button>
        {/if}
      </div>
    {/each}
  </div>

  <section class="detail">
    {#if focused}
      {@const r = focused}
      {@const pct = r.state === "done" ? 100 : fmt.percent(r.done, r.size)}
      <div class="dhead">
        <span class="ic big"><Icon name={ICONS[r.category] ?? "file"} size={22} /></span>
        <div class="dt">
          <b title={r.display}>{r.display}</b>
          <span class="st {r.state}" class:you={needsYou(r) || needsCheck(r)}>{statusText(r)}</span>
        </div>
        <div class="dacts">
          {#if r.state === "downloading" || r.state === "queued"}
            <button class="btn" onclick={() => acts.run(api.pause([r.id]))}><Icon name="pause" size={16} />Pause</button>
          {:else if r.state === "paused" || r.state === "failed"}
            <button class="btn" onclick={() => acts.run(api.resume([r.id]))}><Icon name={r.state === "failed" ? "retry" : "play"} size={16} />{(r.state === "failed" ? "Retry" : "Resume")}</button>
          {:else}
            <button class="btn primary" onclick={() => acts.run(api.open(r.id))}><Icon name="open" size={16} />Open</button>
          {/if}
          {#if r.state !== "done" && r.done > 0 && (r.category === "video" || r.category === "music" || r.kind === "torrent")}
            <button class="btn" onclick={() => acts.run(api.preview(r.id))} title="Watch what has arrived so far"><Icon name="eye" size={16} /></button>
          {/if}
          <button class="btn" onclick={() => acts.run(api.reveal(r.id))} title="Show in folder" aria-label="Show in folder"><Icon name="folder" size={16} /></button>
          <button class="btn danger" onclick={() => acts.remove([r.id])} title="Remove" aria-label="Remove"><Icon name="trash" size={16} /></button>
        </div>
      </div>

      {#if needsYou(r)}
        <div class="fix">
          <span>This file host wants you to wait for its timer or solve a captcha. Do that in a browser window; the app takes the file from there.</span>
          <button class="btn primary" onclick={() => acts.run(api.assist(r.id))}><Icon name="link" size={16} />Get it in a browser window</button>
        </div>
      {:else if needsCheck(r)}
        <div class="fix">
          <span>This site wants a browser check before it lets the download through.</span>
          <button class="btn primary" onclick={() => { acts.run(api.siteCheck(r.url)); acts.say("Pass the check in the new window. The download continues by itself."); }}><Icon name="link" size={16} />Pass site check</button>
          <button class="btn" onclick={() => acts.run(api.browserDownload(r.id))}><Icon name="down" size={16} />Download in a browser window</button>
        </div>
      {:else if r.state === "failed" && r.error}
        <ErrorBox row={r} onsay={acts.say} />
      {/if}

      <div class="bigbar">
        <div class="bar"><i style="width:{pct ?? (r.state === 'downloading' ? 100 : 0)}%" class:indet={pct == null && r.state === "downloading"} class:muted={r.state === "paused" || r.state === "queued"} class:failed={r.state === "failed"}></i></div>
        <b class="mono">{pct != null ? `${pct.toFixed(1)}%` : ""}</b>
      </div>

      {#if dock && grid.length}
        <div class="grid" aria-label="Pieces of the file that have arrived">
          {#each grid as v, i (i)}<i class:h={v === 1} class:n={v === 2} class:w={v === 0.5}></i>{/each}
        </div>
        <div class="legend"><span><i class="h"></i>Arrived</span><span><i class="n"></i>Last minute</span><span><i class="w"></i>Being fetched</span></div>
      {/if}

      <div class="stats">
        <div><span>Size</span><b class="mono">{fmt.bytes(r.size)}</b></div>
        <div><span>Downloaded</span><b class="mono">{fmt.bytes(r.state === "done" ? r.size : r.done)}</b></div>
        <div><span>Speed</span><b class="mono">{r.state === "downloading" ? fmt.speed(r.speed) : "—"}</b></div>
        <div><span>Time left</span><b class="mono">{r.state === "downloading" ? fmt.eta(r.eta) || "estimating" : "—"}</b></div>
        <div><span>{(r.kind === "torrent" ? "Peers" : "Connections")}</span><b class="mono">{r.state === "downloading" ? r.conns : "—"}</b></div>
        <div><span>{(r.state === "done" ? "Finished" : "Added")}</span><b>{fmt.when(r.state === "done" ? r.finished_at : r.created_at)}</b></div>
      </div>

      <div class="tabs-wrap"><DetailsTabs row={r} onsay={acts.say} lean /></div>
    {:else if many.length > 1}
      <div class="multi">
        <b>{many.length} downloads selected</b>
        <span class="muted">{fmt.bytes(many.reduce((a, r) => a + (r.size ?? 0), 0))} in all</span>
        <div class="dacts">
          <button class="btn" onclick={() => acts.run(api.resume(many.map((r) => r.id)))}><Icon name="play" size={16} />Resume</button>
          <button class="btn" onclick={() => acts.run(api.pause(many.map((r) => r.id)))}><Icon name="pause" size={16} />Pause</button>
          <button class="btn" onclick={() => acts.run(api.moveInQueue(many.map((r) => r.id), true))}>To the top</button>
          <button class="btn danger" onclick={() => acts.remove(many.map((r) => r.id))}><Icon name="trash" size={16} />Remove</button>
        </div>
      </div>
    {:else}
      <div class="overview">
        {#if dock}
          <p class="summary"><b>{active.length} arriving</b> · {queued} up next · <span class="mono">{fmt.bytes(toCome)}</span> still to come</p>
        {/if}
        <div class="tiles">
          <div class="tile"><span>Speed now</span><b class="mono">{fmt.speed(speed)}</b></div>
          <div class="tile"><span>Downloading</span><b>{active.length}</b></div>
          <div class="tile"><span>Queued</span><b>{queued}</b></div>
          <div class="tile"><span>Finished today</span><b>{doneToday.length}</b></div>
        </div>
        <div class="graph">
          <div class="gl"><span>Speed, last 2 minutes</span><span class="mono">peak {fmt.speed(peak === 1 ? 0 : peak)}</span></div>
          <svg viewBox="0 0 100 40" preserveAspectRatio="none" aria-hidden="true">
            {#if graph}
              <polygon points="0,40 {graph} {((samples.length - 1) / 119 * 100).toFixed(2)},40" class="area" />
              <polyline points={graph} class="line" />
            {/if}
          </svg>
        </div>
        {#if dock && upNext.length}
          <div class="next">
            <h3>Up next <span class="mono">starts when a slot frees</span></h3>
            {#each upNext as q (q.id)}
              <div class="q"><Icon name={ICONS[q.category] ?? "file"} size={16} /><b title={q.display}>{q.display}</b><span class="mono">{fmt.bytes(q.size)}</span></div>
            {/each}
          </div>
        {/if}
        <p class="muted">{(all.length ? "Choose a download on the left to see it here" : "Your downloads show up here")}</p>
        {#if !all.length}
          <div class="dacts start">
            <button class="btn primary" onclick={acts.add}><Icon name="plus" size={16} />Add a link</button>
            <button class="btn" onclick={acts.extension}><Icon name="link" size={16} />Set up the browser extension</button>
          </div>
        {/if}
      </div>
    {/if}
  </section>
</div>

<style>
  .split { display: grid; grid-template-columns: minmax(300px, 380px) 1fr; height: 100%; min-height: 0; }
  .mono { font-family: var(--mono); font-variant-numeric: tabular-nums; }
  .muted { color: var(--muted); }

  .list { overflow: auto; padding: 12px 8px; display: flex; flex-direction: column; gap: 4px; border-right: 1px solid var(--line); }
  /* Off-screen cards aren't drawn (thousands stay smooth). */
  .card { content-visibility: auto; contain-intrinsic-size: auto 84px; }
  .card { display: flex; gap: 12px; padding: 12px; border-radius: 12px; border: 1px solid transparent; cursor: default; transition: background var(--t-fast) var(--ease), border-color var(--t-fast) var(--ease); }
  .card:hover { background: var(--card-hover); }
  .card.sel { background: var(--card); border-color: var(--line3); box-shadow: inset 2px 0 0 var(--sel-edge); }
  .ic { width: 40px; height: 40px; border-radius: 10px; border: 1px solid var(--line2); display: grid; place-items: center; color: var(--text-2); flex: none; background: var(--card); }
  .ic.big { width: 48px; height: 48px; border-radius: 12px; }
  .body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 6px; }
  .l1 { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .l1 b { flex: 1; min-width: 0; font-size: 14px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .card.done .l1 b { color: var(--text-2); }
  .l2 { font-size: 12.5px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .card.failed .l2 { color: var(--down); font-family: var(--font); }
  .st { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--muted); white-space: nowrap; flex: none; }
  .st::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: currentColor; opacity: 0.8; }
  .st.downloading { color: var(--up); }
  .st.downloading::before { box-shadow: 0 0 0 3px var(--up-soft); opacity: 1; }
  .st.failed, .st.you { color: var(--down); }
  .pbar { height: 6px; border-radius: 3px; background: var(--line2); overflow: hidden; }
  .pbar i, .bar i { display: block; height: 100%; background: var(--bar); border-radius: 3px; transition: width 450ms linear; }
  .card.paused .pbar i, .card.queued .pbar i { background: var(--muted); }
  .card.failed .pbar i { background: var(--down); }
  .indet { opacity: 0.45; animation: pulse 1.2s ease-in-out infinite; }
  .none { margin: 40px auto; display: flex; flex-direction: column; align-items: center; gap: 8px; color: var(--muted); text-align: center; font-size: 14px; }
  .none b { color: var(--text); font-size: 15px; }

  .detail { overflow: auto; padding: 24px 32px; display: flex; flex-direction: column; gap: 16px; min-width: 0; }
  .dhead { display: flex; align-items: center; gap: 16px; min-width: 0; }
  .dt { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 4px; align-items: flex-start; }
  .dt b { max-width: 100%; font-size: 18px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dacts { display: flex; gap: 8px; flex-wrap: wrap; }
  .dacts.start { margin-top: 8px; }
  .btn { height: 36px; padding: 0 16px; border-radius: 8px; border: 1px solid var(--line2); background: transparent; color: var(--text-2); font-size: 14px; display: inline-flex; align-items: center; gap: 7px; white-space: nowrap; }
  .btn:hover { border-color: var(--line3); color: var(--text); }
  .btn.primary { background: var(--hero); color: var(--hero-text); border-color: var(--hero); font-weight: 500; }
  .btn.primary:hover { color: var(--hero-text); filter: brightness(0.92); }
  .btn.danger { color: var(--down); border-color: color-mix(in srgb, var(--down) 35%, transparent); }
  .fix { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; padding: 12px 14px; border: 1px solid var(--line2); border-radius: 12px; font-size: 13.5px; color: var(--text-2); }
  .fix span { flex: 1; min-width: 220px; }
  .fix.bad { color: var(--down); border-color: color-mix(in srgb, var(--down) 35%, transparent); user-select: text; }
  .bigbar { display: flex; align-items: center; gap: 14px; }
  .bar { flex: 1; height: 14px; border-radius: 7px; background: var(--line2); overflow: hidden; }
  .bar i { border-radius: 7px; }
  .bar i.muted { background: var(--muted); }
  .bar i.failed { background: var(--down); }
  .bigbar b { width: 70px; text-align: right; font-size: 16px; }
  .stats { display: grid; grid-template-columns: repeat(3, 1fr); gap: 16px 24px; padding: 16px 20px; border: 1px solid var(--line); border-radius: 12px; background: var(--card); }
  .stats div { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .stats span { font-size: 12px; color: var(--muted); }
  .stats b { font-size: 15px; font-weight: 500; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tabs-wrap { flex: 1; min-height: 260px; display: flex; flex-direction: column; font-size: 13.5px; }

  .multi { display: flex; flex-direction: column; gap: 10px; align-items: flex-start; }
  .multi b { font-size: 18px; }
  .overview { display: flex; flex-direction: column; gap: 16px; }
  .tiles { display: grid; grid-template-columns: repeat(auto-fill, minmax(170px, 1fr)); gap: 12px; }
  .tile { padding: 16px 20px; border: 1px solid var(--line); border-radius: 12px; background: var(--card); display: flex; flex-direction: column; gap: 6px; }
  .tile span { font-size: 12.5px; color: var(--muted); }
  .tile b { font-size: 24px; font-weight: 500; }
  .overview p { margin: 0; font-size: 14px; }
  .graph { border: 1px solid var(--line); border-radius: 12px; background: var(--card); padding: 12px 14px 8px; }
  .gl { display: flex; justify-content: space-between; font-size: 12px; color: var(--muted); margin-bottom: 6px; }
  .graph svg { width: 100%; height: 110px; display: block; }
  .graph .line { fill: none; stroke: var(--bar); stroke-width: 0.6; vector-effect: non-scaling-stroke; }
  .graph .line { stroke-width: 2; }
  .graph .area { fill: var(--bar-soft); stroke: none; }
  .tags { display: flex; gap: 4px; flex-wrap: wrap; }
  .tags span { font: 12px var(--mono); color: var(--muted); }

  /* Dock theme pieces */
  .strip { display: flex; gap: 2px; height: 4px; margin: 1px 0; }
  .strip i { flex: 1; border-radius: 1px; background: var(--line2); position: relative; overflow: hidden; }
  .strip i::after { content: ""; position: absolute; inset: 0 auto 0 0; width: var(--f); background: var(--text-2); transition: width 450ms linear; }
  .card.sel .strip i::after { background: var(--bar); }
  .grid { display: grid; grid-template-columns: repeat(64, 1fr); gap: 2px; padding: 16px; border-radius: 12px; background: var(--card); border: 1px solid var(--line); }
  .grid i { aspect-ratio: 1; border-radius: 2px; background: var(--line2); }
  .grid i.h, .legend i.h { background: #c47d22; }
  .grid i.n, .legend i.n { background: var(--bar); }
  .grid i.w, .legend i.w { background: #6b4a1f; }
  .legend { display: flex; gap: 16px; font-size: 12px; color: var(--muted); margin-top: -8px; }
  .legend span { display: inline-flex; align-items: center; gap: 6px; }
  .legend i { width: 8px; height: 8px; border-radius: 2px; display: inline-block; }
  .summary { color: var(--text-2); }
  .summary b { color: var(--text); font-weight: 500; }
  .next { border: 1px solid var(--line); border-radius: 12px; background: var(--card); padding: 16px 20px 8px; }
  .next h3 { margin: 0 0 8px; font-size: 14px; font-weight: 400; color: var(--text-2); display: flex; justify-content: space-between; }
  .next h3 span { font-size: 12px; color: var(--muted); }
  .q { display: flex; align-items: center; gap: 10px; height: 36px; border-top: 1px solid var(--line); color: var(--text-2); }
  .q b { flex: 1; min-width: 0; font-weight: 400; color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .q span { font-size: 12px; color: var(--muted); }

  @keyframes pulse { 50% { opacity: 0.15; } }
  @media (max-width: 1100px) { .stats { grid-template-columns: repeat(2, 1fr); } }
</style>
