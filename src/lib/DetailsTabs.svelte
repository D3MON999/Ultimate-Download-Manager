<script lang="ts">
  import ServerFlag from "$lib/ServerFlag.svelte";
  // The details panel's tabs for one download: a torrent's Content (its files
  // as a folder tree, each with its progress and a box to download it or not), Peers, Trackers
  // and Info (pieces); a file's Info and Connections (its parts as the engine splits them).
  import Icon from "$lib/Icon.svelte";
  import { api, inTauri, type ContentFile, type Details, type ItemExtras, type Row } from "$lib/api";
  import { open as pickPath } from "@tauri-apps/plugin-dialog";
  import * as fmt from "$lib/format";
  import { onDestroy } from "svelte";

  let { row, onsay, lean = false }: { row: Row; onsay: (text: string, bad?: boolean) => void; lean?: boolean } = $props();

  type Tab = "content" | "peers" | "trackers" | "info" | "connections" | "options";
  const isTorrent = $derived(row.kind === "torrent");
  const tabs = $derived<[Tab, string][]>(
    isTorrent
      ? [["content", "Content"], ["peers", "Peers"], ["trackers", "Trackers"], ["info", "Info"], ["options", "Options"]]
      : row.kind === "file"
        ? [["info", "Info"], ["connections", "Connections"], ["options", "Options"]]
        : [["info", "Info"], ["options", "Options"]],
  );
  let tab = $state<Tab>("info");
  let details = $state<Details | null>(null);
  let failed = $state("");
  let shownFor = -1;
  let busy = $state(false);

  // A new row: its first tab, its details again.
  $effect(() => {
    if (row.id !== shownFor) {
      shownFor = row.id;
      tab = row.kind === "torrent" ? "content" : "info";
      details = null;
      failed = "";
      load();
    }
  });

  async function load() {
    const id = row.id;
    if (!inTauri) {
      details = demo(row);
      return;
    }
    try {
      const d = await api.details(id);
      if (id === row.id) {
        details = d;
        failed = "";
      }
    } catch (e) {
      if (id === row.id) failed = String(e);
    }
  }
  const timer = setInterval(() => {
    if (row.state === "downloading" || details == null) load();
  }, 1000);
  onDestroy(() => clearInterval(timer));

  const torrent = $derived(details?.kind === "torrent" ? details.torrent : null);
  const file = $derived(details?.kind === "file" ? details.file : null);
  const extras = $derived<ItemExtras | null>(details?.item ?? null);
  const sequential = $derived(details?.kind === "torrent" ? details.sequential : false);

  // ---- Options
  let limitIn = $state("");
  let checksumIn = $state("");
  let checksumSaid = $state("");
  let passwordIn = $state("");
  let addressIn = $state("");
  let renameIn = $state("");
  let trackersIn = $state("");
  let tagsIn = $state("");
  let mirrorIn = $state("");
  let seedRatioIn = $state("");
  let seedMinutesIn = $state("");
  let presets = $state<[string, string][]>([]);
  let preset = $state("mp3");
  let converting = $state(false);
  let optionsFor = -1;
  $effect(() => {
    if (extras && optionsFor !== row.id) {
      optionsFor = row.id;
      limitIn = extras.limit_kib ? String(extras.limit_kib) : "";
      checksumIn = extras.checksum ?? "";
      checksumSaid = "";
      passwordIn = "";
      addressIn = "";
      renameIn = row.display;
      trackersIn = "";
      tagsIn = row.tags ?? "";
      mirrorIn = "";
      seedRatioIn = extras.seed_ratio != null ? String(extras.seed_ratio) : "";
      seedMinutesIn = extras.seed_minutes != null ? String(extras.seed_minutes) : "";
    }
  });
  $effect(() => {
    if (tab === "options" && !presets.length && inTauri) api.convertPresets().then((p) => (presets = p));
  });

  async function act(p: Promise<unknown>, done?: string) {
    try {
      await p;
      if (done) onsay(done);
      await load();
    } catch (e) {
      onsay(String(e), true);
    }
  }

  async function saveChecksum() {
    try {
      const ok = await api.setChecksum(row.id, checksumIn);
      checksumSaid = ok == null ? (checksumIn.trim() ? "Checked when it finishes." : "") : ok ? "✓ The file matches." : "✕ The file doesn't match: it's damaged or a different file.";
    } catch (e) {
      checksumSaid = String(e);
    }
  }

  async function moveTo() {
    const dir = await pickPath({ directory: true, title: "Move the file to" });
    if (typeof dir === "string") act(api.moveFile(row.id, dir), "Moved");
  }

  async function convert() {
    converting = true;
    try {
      const out = await api.convertFile(row.id, preset);
      onsay(`Converted: ${out.split(/[\\/]/).pop()}`);
    } catch (e) {
      onsay(String(e), true);
    } finally {
      converting = false;
    }
  }

  // ---- Content: the files as a folder tree
  type Node = { name: string; key: string; dirs: Node[]; files: ContentFile[]; size: number; done: number | null; want: number; total: number };
  let closed = $state<Set<string>>(new Set());

  const tree = $derived.by(() => {
    const root: Node = { name: "", key: "", dirs: [], files: [], size: 0, done: 0, want: 0, total: 0 };
    for (const f of torrent?.files ?? []) {
      const parts = f.path.split(/[\\/]/).filter(Boolean);
      let at = root;
      const chain: Node[] = [root];
      for (const p of parts.slice(0, -1)) {
        let next = at.dirs.find((d) => d.name === p);
        if (!next) {
          next = { name: p, key: `${at.key}/${p}`, dirs: [], files: [], size: 0, done: 0, want: 0, total: 0 };
          at.dirs.push(next);
        }
        at = next;
        chain.push(at);
      }
      at.files.push(f);
      for (const n of chain) {
        n.size += f.size;
        n.done = n.done == null || f.done == null ? null : n.done + f.done;
        n.total += 1;
        if (f.wanted) n.want += 1;
      }
    }
    // A torrent of one folder: show what's in it.
    return root.files.length === 0 && root.dirs.length === 1 ? root.dirs[0] : root;
  });

  const filesUnder = (n: Node): ContentFile[] => [...n.files, ...n.dirs.flatMap(filesUnder)];

  async function setWanted(changed: ContentFile[], want: boolean) {
    if (!torrent || busy) return;
    const ids = new Set(changed.map((f) => f.index));
    const next = torrent.files.filter((f) => (ids.has(f.index) ? want : f.wanted)).map((f) => f.index);
    if (!next.length) return onsay("Keep at least one file. To stop the whole torrent, pause or remove it.", true);
    busy = true;
    try {
      if (inTauri) await api.torrentChoose(row.id, next);
      for (const f of torrent.files) f.wanted = next.includes(f.index);
      if (details?.kind === "torrent") details = { ...details, torrent: { ...torrent } };
      await load();
    } catch (e) {
      onsay(String(e), true);
    } finally {
      busy = false;
    }
  }

  const pct = (done: number | null, size: number) => (done == null ? null : size ? Math.min(100, (done / size) * 100) : 100);

  function toggleDir(key: string) {
    const s = new Set(closed);
    s.has(key) ? s.delete(key) : s.add(key);
    closed = s;
  }

  // ---- sample data for the browser preview (?demo)
  const DEMO_EXTRAS: ItemExtras = { limit_kib: 0, priority: 0, checksum: null, password: false };
  function demo(r: Row): Details {
    if (r.kind !== "torrent") {
      const size = r.size ?? 100;
      const parts = 8;
      const segments: [number, number, number][] = Array.from({ length: parts }, (_, i) => {
        const start = Math.floor((size / parts) * i), end = Math.floor((size / parts) * (i + 1));
        return [start, start + Math.floor((end - start) * [1, 0.82, 0.64, 0.51, 0.33, 0.2, 0.08, 1][i]), end];
      });
      return { kind: "file", item: DEMO_EXTRAS, file: { segments, connections: segments.map(([s, c], i) => [c - s, i === 0 || i === 7 ? 0 : 2_300_000 + i * 90_000, i !== 0 && i !== 7]), resumable: true, server: { ip: "88.198.54.17", country: "DE", flag: [256, 72], rtt_ms: 24, proxied: false, cdn: null } } };
    }
    const files: ContentFile[] = [
      ["debian-12.7.0-amd64-DVD-1.iso", 3_994_091_520, 0.23],
      ["SHA256SUMS", 1_024, 1],
      ["SHA256SUMS.sign", 833, 1],
      ["docs/README.html", 52_400, 1],
      ["docs/install-guide/en/index.html", 211_000, 0.6],
      ["docs/install-guide/en/ch01.html", 88_000, 0],
      ["extras/firmware-nonfree.tar.gz", 412_000_000, 0],
    ].map(([path, size, p], index) => ({ index, path: `debian-12.7.0/${path}`, size: size as number, done: Math.floor((size as number) * (p as number)), wanted: index !== 6 }));
    return {
      kind: "torrent",
      sequential: false,
      item: DEMO_EXTRAS,
      torrent: {
        hash: "f0a4c1a2b3d4e5f60718293a4b5c6d7e8f901234",
        live: true,
        files,
        peers: [
          { addr: "185.21.216.4:51413", client: "Transmission 4.0.6", fetched: 412_000_000, uploaded: 1_200_000, kind: "tcp" },
          { addr: "91.189.88.12:6881", client: "Transmission 4.0.6", fetched: 288_000_000, uploaded: 0, kind: "utp" },
          { addr: "[2a01:4f8::1]:6881", client: "libtorrent (Rasterbar) 2.0", fetched: 120_500_000, uploaded: 340_000, kind: "tcp" },
          { addr: "77.68.2.9:49160", client: null, fetched: 91_000_000, uploaded: 0, kind: "tcp" },
        ],
        peers_seen: 64,
        trackers: ["http://bttracker.debian.org:6969/announce"],
        pieces: Array.from({ length: 300 }, (_, i) => (i < 40 ? 1 : i < 90 ? (i % 3 ? 1 : 0.5) : i % 11 === 0 ? 1 : 0)),
        piece_count: 15237,
        piece_size: 262_144,
        uploaded: 1_540_000,
        up_speed: 42_000,
        phase: "Downloading",
      },
    };
  }
</script>

<div class="tabs" role="tablist">
  {#each tabs as [id, label] (id)}
    <button role="tab" aria-selected={tab === id} class:on={tab === id} onclick={() => (tab = id)}>
      {(label)}
      {#if id === "content" && torrent}<em>{torrent.files.length}</em>{/if}
      {#if id === "peers" && torrent}<em>{torrent.peers.length}</em>{/if}
      {#if id === "connections" && file}<em>{file.connections.filter((c) => c[2]).length}</em>{/if}
    </button>
  {/each}
</div>

<div class="pane">
  {#if failed}
    <p class="muted">{failed}</p>
  {:else if tab === "info"}
    <dl class="facts">
      {#if !lean}<div><dt>Downloaded</dt><dd class="mono">{fmt.bytes(row.state === "done" ? row.size : row.done)}{row.size && row.state !== "done" ? ` of ${fmt.bytes(row.size)}` : ""}</dd></div>{/if}
      {#if row.state === "downloading" && !lean}
        <div><dt>Speed</dt><dd class="mono">{fmt.speed(row.speed)}</dd></div>
        <div><dt>Time left</dt><dd class="mono">{fmt.eta(row.eta) || "estimating"}</dd></div>
      {/if}
      {#if torrent}
        <div><dt>Uploaded</dt><dd class="mono">{fmt.bytes(torrent.uploaded)}{torrent.up_speed ? ` · ${fmt.speed(torrent.up_speed)}` : ""}</dd></div>
        <div><dt>Peers</dt><dd class="mono">{torrent.peers.length} connected · {torrent.peers_seen} seen</dd></div>
        {#if torrent.piece_count}<div><dt>Pieces</dt><dd class="mono">{torrent.piece_count} × {fmt.bytes(torrent.piece_size)}</dd></div>{/if}
      {/if}
      {#if file?.server}<div class="wide"><dt>{file.server.proxied ? "Proxy" : "Server"}</dt><dd class="path"><ServerFlag server={file.server} /></dd></div>{/if}
      {#if file?.resumable != null}<div><dt>Resumable</dt><dd>{file.resumable ? "Yes" : "No, the server can't continue a stopped download"}</dd></div>{/if}
      {#if !lean}<div><dt>Added</dt><dd>{fmt.when(row.created_at)}</dd></div>{/if}
      {#if row.finished_at}<div><dt>Finished</dt><dd>{fmt.when(row.finished_at)}</dd></div>{/if}
      <div class="wide"><dt>Saved to</dt><dd class="mono path" title={row.file ?? row.dir}>{row.file ?? row.dir}</dd></div>
      <div class="wide"><dt>{torrent ? "Magnet" : "Address"}</dt><dd class="mono path" title={row.url}>{row.url}</dd></div>
      {#if extras?.rule}<div class="wide"><dt>Your rule</dt><dd>{extras.rule}</dd></div>{/if}
      {#if torrent}<div class="wide"><dt>Info hash</dt><dd class="mono path">{torrent.hash}</dd></div>{/if}
      {#if row.referer}<div class="wide"><dt>From page</dt><dd class="mono path" title={row.referer}>{row.referer}</dd></div>{/if}
    </dl>
    {#if torrent && torrent.pieces.length}
      <div class="label">Pieces</div>
      <div class="pieces" aria-label="Pieces downloaded">
        {#each torrent.pieces as p, i (i)}<i style="opacity:{0.12 + p * 0.88}" class:full={p === 1}></i>{/each}
      </div>
    {/if}
  {:else if tab === "content"}
    {#if !torrent}
      <p class="muted">Reading the torrent's files…</p>
    {:else}
      <div class="opt-row tbar">
        <button class="obtn" onclick={() => act(api.torrentAction(row.id, "recheck"), "Checking every piece again")} title="Read every piece on disk again (after moving or repairing files)">Recheck</button>
        <button class="obtn" onclick={() => act(api.torrentAction(row.id, "reannounce"), "Asking for peers again")} title="Ask the trackers and DHT for peers now">Reannounce</button>
        <label class="check small"><input type="checkbox" checked={sequential} onchange={(e) => act(api.torrentAction(row.id, e.currentTarget.checked ? "sequential-on" : "sequential-off"))} /> Download in order (to watch while it downloads)</label>
      </div>
      <div class="ctable" role="treegrid" aria-label="Files in this torrent">
        <div class="chead"><span>Name</span><span class="num">Size</span><span>Progress</span><span class="num">Left</span></div>
        {#snippet node(n: Node, depth: number)}
          {#each n.dirs as d (d.key)}
            {@const all = d.want === d.total}
            {@const some = d.want > 0 && !all}
            {@const p = pct(d.done, d.size)}
            <div class="crow dir" style="--d:{depth}">
              <span class="cname">
                <button class="twist" onclick={() => toggleDir(d.key)} aria-label={closed.has(d.key) ? `Open ${d.name}` : `Close ${d.name}`}>{closed.has(d.key) ? "▸" : "▾"}</button>
                <input type="checkbox" checked={all} indeterminate={some} disabled={busy} onchange={(e) => setWanted(filesUnder(d), e.currentTarget.checked)} aria-label="Download {d.name}" />
                <Icon name="folder" size={14} />
                <span class="fname" title={d.name}>{d.name}</span>
              </span>
              <span class="num mono">{fmt.bytes(d.size)}</span>
              <span class="cprog">
                {#if d.want === 0}<span class="muted small">Not downloaded</span>
                {:else if p != null}<span class="bar" class:done={p >= 100}><i style="width:{p}%"></i></span><span class="mono pc">{p.toFixed(1)}%</span>{/if}
              </span>
              <span class="num mono">{d.want && d.done != null ? fmt.bytes(Math.max(0, d.size - d.done)) : ""}</span>
            </div>
            {#if !closed.has(d.key)}{@render node(d, depth + 1)}{/if}
          {/each}
          {#each n.files as f (f.index)}
            {@const p = pct(f.done, f.size)}
            {@const name = f.path.split(/[\\/]/).pop()}
            <div class="crow" class:skip={!f.wanted} style="--d:{depth}">
              <span class="cname">
                <span class="twist"></span>
                <input type="checkbox" checked={f.wanted} disabled={busy} onchange={(e) => setWanted([f], e.currentTarget.checked)} aria-label="Download {name}" />
                <Icon name="file" size={14} />
                <span class="fname" title={f.path}>{name}</span>
              </span>
              <span class="num mono">{fmt.bytes(f.size)}</span>
              <span class="cprog">
                {#if !f.wanted}<span class="muted small">Not downloaded</span>
                {:else if p != null}<span class="bar" class:done={p >= 100}><i style="width:{p}%"></i></span><span class="mono pc">{p.toFixed(1)}%</span>{/if}
              </span>
              <span class="num mono">{f.wanted && f.done != null ? fmt.bytes(Math.max(0, f.size - f.done)) : ""}</span>
            </div>
          {/each}
        {/snippet}
        {@render node(tree, 0)}
      </div>
      {#if !torrent.live}<p class="muted small">Progress shows while the torrent runs.</p>{/if}
    {/if}
  {:else if tab === "peers"}
    {#if !torrent?.peers.length}
      <p class="muted">{torrent?.live ? "Looking for peers…" : "Peers show while the torrent runs."}</p>
    {:else}
      <div class="ptable">
        <div class="phead"><span>Address</span><span>Client</span><span>Type</span><span class="num">Received</span><span class="num">Sent</span></div>
        {#each torrent.peers as p (p.addr)}
          <div class="prow">
            <span class="mono path">{p.addr}</span>
            <span class="path">{p.client ?? "Unknown"}</span>
            <span class="muted">{p.kind?.toUpperCase() ?? ""}</span>
            <span class="num mono">{fmt.bytes(p.fetched)}</span>
            <span class="num mono">{fmt.bytes(p.uploaded)}</span>
          </div>
        {/each}
      </div>
    {/if}
  {:else if tab === "trackers"}
    {#if !torrent?.trackers.length}
      <p class="muted">{torrent?.live ? "This torrent has no trackers: peers come from DHT and other peers." : "Trackers show while the torrent runs."}</p>
    {:else}
      <ul class="trackers">{#each torrent.trackers as t (t)}<li class="mono path" title={t}>{t}</li>{/each}</ul>
    {/if}
    <div class="label">Add trackers (one address per line)</div>
    <textarea rows="3" bind:value={trackersIn} placeholder="udp://tracker.opentrackr.org:1337/announce" spellcheck="false"></textarea>
    <div class="opt-row"><button class="obtn" disabled={!trackersIn.trim()} onclick={() => act(api.torrentAddTrackers(row.id, trackersIn).then((n) => { trackersIn = ""; onsay(`${n} tracker${n === 1 ? "" : "s"} added`); }))}>Add trackers</button></div>
  {:else if tab === "options"}
    <div class="opts">
      <div class="opt">
        <div class="label">Tags (words; filter with #tag)</div>
        <div class="opt-row">
          <input class="oin" bind:value={tagsIn} placeholder="work linux" spellcheck="false" />
          <button class="obtn" onclick={() => act(api.setTags([row.id], tagsIn), "Tags saved")}>Save</button>
        </div>
      </div>
      {#if row.kind === "torrent"}
        <div class="opt">
          <div class="label">Stop sharing this torrent at (empty = the settings')</div>
          <div class="opt-row">
            <input class="oin narrow" type="number" min="0" step="0.1" bind:value={seedRatioIn} placeholder="ratio" />
            <input class="oin narrow" type="number" min="0" bind:value={seedMinutesIn} placeholder="minutes" />
            <button class="obtn" onclick={() => act(api.torrentSeeding(row.id, seedRatioIn === "" ? null : Number(seedRatioIn), seedMinutesIn === "" ? null : Number(seedMinutesIn)), "Seeding limits saved")}>Apply</button>
          </div>
        </div>
      {/if}
      {#if row.kind === "file"}
        <div class="opt">
          <div class="label">Mirrors: other addresses for the same file, tried when this one fails{extras?.mirrors?.length ? ` (${extras.mirrors.length})` : ""}</div>
          {#each extras?.mirrors ?? [] as mu (mu)}<div class="mono small mirror" title={mu}>{mu}</div>{/each}
          <div class="opt-row">
            <input class="oin" bind:value={mirrorIn} placeholder="https://another-site/same-file.rar" spellcheck="false" />
            <button class="obtn" disabled={!mirrorIn.trim()} onclick={() => act(api.addMirrors(row.id, [mirrorIn]).then(() => (mirrorIn = "")), "Mirror added")}>Add</button>
          </div>
        </div>
      {/if}
      <div class="opt">
        <div class="label">Speed limit for this download (KB/s, empty = none)</div>
        <div class="opt-row">
          <input class="oin narrow" type="number" min="0" bind:value={limitIn} placeholder="None" />
          <button class="obtn" onclick={() => act(api.setItemLimit([row.id], Number(limitIn) || 0), Number(limitIn) ? `Limited to ${limitIn} KB/s` : "No limit")}>Apply</button>
        </div>
      </div>
      {#if row.state !== "done"}
        <div class="opt">
          <div class="label">Queue</div>
          <div class="opt-row">
            <button class="obtn" onclick={() => act(api.moveInQueue([row.id], true), "Moved to the top")}>To the top</button>
            <button class="obtn" onclick={() => act(api.nudge(row.id, true))}>Up</button>
            <button class="obtn" onclick={() => act(api.nudge(row.id, false))}>Down</button>
            <button class="obtn" onclick={() => act(api.moveInQueue([row.id], false), "Moved to the bottom")}>To the bottom</button>
          </div>
        </div>
      {/if}
      {#if row.kind === "file"}
        <div class="opt">
          <div class="label">Checksum to verify (MD5, SHA-1, SHA-256 or SHA-512)</div>
          <div class="opt-row">
            <input class="oin" bind:value={checksumIn} placeholder="sha256:… or paste the hex digest" spellcheck="false" />
            <button class="obtn" onclick={saveChecksum}>{row.state === "done" ? "Check" : "Set"}</button>
          </div>
          {#if checksumSaid}<p class="small" class:good={checksumSaid.startsWith("✓")} class:bad={checksumSaid.startsWith("✕")}>{checksumSaid}</p>{/if}
        </div>
        <div class="opt">
          <div class="label">Archive password {extras?.password ? "(one is set)" : ""}</div>
          <div class="opt-row">
            <input class="oin" type="password" bind:value={passwordIn} placeholder="Used when it's unpacked" autocomplete="off" />
            <button class="obtn" onclick={() => act(api.setArchivePassword(row.id, passwordIn), passwordIn ? "Password saved" : "Password removed")}>Save</button>
          </div>
        </div>
        {#if row.state === "paused" || row.state === "failed"}
          <div class="opt">
            <div class="label">New address for the same file (the link expired); what's downloaded is kept</div>
            <div class="opt-row">
              <input class="oin" bind:value={addressIn} placeholder="https://…" spellcheck="false" />
              <button class="obtn" disabled={!addressIn.trim()} onclick={() => act(api.refreshAddress(row.id, addressIn), "Resuming from the new address")}>Use it</button>
            </div>
          </div>
        {/if}
      {/if}
      {#if row.state === "done" && row.file}
        <div class="opt">
          <div class="label">Rename or move</div>
          <div class="opt-row">
            <input class="oin" bind:value={renameIn} spellcheck="false" />
            <button class="obtn" disabled={!renameIn.trim() || renameIn === row.display} onclick={() => act(api.moveFile(row.id, renameIn), "Renamed")}>Rename</button>
            <button class="obtn" onclick={moveTo}>Move to…</button>
          </div>
        </div>
        {#if row.category === "video" || row.category === "music"}
          <div class="opt">
            <div class="label">Convert (a new file beside it; this one stays)</div>
            <div class="opt-row">
              <select class="oin" bind:value={preset}>{#each presets as [k, label] (k)}<option value={k}>{label}</option>{/each}</select>
              <button class="obtn" disabled={converting} onclick={convert}>{converting ? "Converting…" : "Convert"}</button>
            </div>
          </div>
        {/if}
      {/if}
    </div>
  {:else if tab === "connections"}
    {#if !file}
      <p class="muted">{row.state === "downloading" ? "Starting…" : "Its parts and connections show while it downloads."}</p>
    {:else}
      {@const total = file.segments.length ? file.segments[file.segments.length - 1][2] : 0}
      <div class="label">Parts ({file.segments.length})</div>
      <div class="segmap" aria-label="Parts downloaded">
        {#each file.segments as [s, c, e], i (i)}
          <span style="flex:{Math.max(1, e - s)}"><i style="width:{e > s ? ((c - s) / (e - s)) * 100 : 100}%"></i></span>
        {/each}
      </div>
      <div class="ptable conns">
        <div class="phead"><span>#</span><span>Part</span><span class="num">Received</span><span class="num">Speed</span></div>
        {#each file.connections as [got, speed, on], i (i)}
          {@const seg = file.segments[i]}
          <div class="prow">
            <span class="mono">{i + 1}</span>
            <span class="mono muted">{seg && total ? `${((seg[0] / total) * 100).toFixed(0)}–${((seg[2] / total) * 100).toFixed(0)}%` : ""}</span>
            <span class="num mono">{fmt.bytes(got)}</span>
            <span class="num mono" class:muted={!on}>{on ? fmt.speed(speed) : "idle"}</span>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .tabs { display: flex; gap: 2px; border-bottom: 1px solid var(--line); }
  .tabs button { position: relative; height: 32px; padding: 0 12px; border: 0; background: none; color: var(--muted); font-size: 12.5px; display: inline-flex; align-items: center; gap: 6px; }
  .tabs button:hover { color: var(--text); }
  .tabs button.on { color: var(--text); }
  .tabs button.on::after { content: ""; position: absolute; left: 8px; right: 8px; bottom: -1px; height: 2px; background: var(--text); border-radius: 2px; }
  .tabs em { font: normal 12px var(--mono); color: var(--muted); }
  .pane { flex: 1; min-height: 0; overflow: auto; padding-top: 10px; }
  .muted { color: var(--muted); }
  .small { font-size: 11.5px; }
  .mono { font-family: var(--mono); font-variant-numeric: tabular-nums; }
  .path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; user-select: text; }
  p { margin: 4px 0; }
  .label { margin: 12px 0 6px; font-size: 11px; color: var(--muted); letter-spacing: 0.04em; }

  .facts { display: grid; grid-template-columns: repeat(auto-fill, minmax(170px, 1fr)); gap: 8px var(--s4); margin: 0; }
  .facts > div { min-width: 0; }
  .facts .wide { grid-column: 1 / -1; display: grid; grid-template-columns: 90px 1fr; align-items: baseline; gap: var(--s3); }
  .facts dt { font-size: 11px; color: var(--muted); letter-spacing: 0.04em; }
  .facts dd { margin: 2px 0 0; font-size: 12.5px; color: var(--text-2); }
  .facts dd.path { margin: 0; }

  .pieces { display: flex; height: 14px; gap: 0; border-radius: 3px; overflow: hidden; background: var(--line); }
  .pieces i { flex: 1; background: var(--bar); }

  /* Content */
  .ctable, .ptable { font-size: 12.5px; }
  .chead, .crow { display: grid; grid-template-columns: minmax(0, 1fr) 96px 170px 90px; align-items: center; gap: 10px; }
  .chead, .phead { height: 26px; color: var(--muted); font-size: 11px; letter-spacing: 0.04em; border-bottom: 1px solid var(--line); position: sticky; top: -10px; background: var(--card); z-index: 1; }
  .crow { height: 28px; border-bottom: 1px solid var(--line); color: var(--text-2); }
  .crow:hover { background: var(--card-hover); }
  .crow.skip { color: var(--muted); }
  .crow.dir { color: var(--text); }
  .cname { display: flex; align-items: center; gap: 6px; min-width: 0; padding-left: calc(var(--d) * 18px); }
  .fname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .twist { width: 16px; flex: none; border: 0; background: none; color: var(--muted); padding: 0; font-size: 11px; text-align: center; }
  .cname input { width: 14px; height: 14px; margin: 0; accent-color: var(--text); flex: none; }
  .num { text-align: right; }
  .cprog { display: flex; align-items: center; gap: 8px; }
  .bar { flex: 1; height: 5px; border-radius: 3px; background: var(--line2); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--bar); transition: width 400ms linear; }
  .bar.done i { background: var(--text-2); }
  .pc { width: 44px; text-align: right; font-size: 11px; color: var(--muted); }

  /* Peers, connections */
  .phead, .prow { display: grid; grid-template-columns: minmax(0, 1.3fr) minmax(0, 1fr) 54px 100px 100px; align-items: center; gap: 10px; }
  .conns .phead, .conns .prow { grid-template-columns: 40px minmax(0, 1fr) 110px 110px; }
  .prow { height: 28px; border-bottom: 1px solid var(--line); color: var(--text-2); }
  .trackers { list-style: none; margin: 0; padding: 0; }
  .trackers li { padding: 7px 0; border-bottom: 1px solid var(--line); color: var(--text-2); font-size: 12.5px; }
  .segmap { display: flex; gap: 2px; height: 14px; }
  .opts { display: grid; grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); gap: 14px 24px; }
  .opt .label { margin-top: 0; }
  .opt-row { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
  .opt-row.tbar { margin-bottom: 8px; }
  .oin { flex: 1; min-width: 120px; height: 30px; padding: 0 10px; border: 1px solid var(--line2); border-radius: 7px; background: var(--field); color: var(--text); font: inherit; font-size: 12.5px; outline: none; }
  .oin:focus { border-color: var(--text-2); }
  .oin.narrow { flex: 0 0 110px; }
  .obtn { height: 30px; padding: 0 12px; border: 1px solid var(--line2); border-radius: 7px; background: transparent; color: var(--text-2); font-size: 12.5px; }
  .obtn:hover:not(:disabled) { border-color: var(--line3); color: var(--text); }
  .obtn:disabled { opacity: 0.4; }
  .check { display: inline-flex; align-items: center; gap: 6px; color: var(--text-2); }
  .check input { accent-color: var(--text); }
  textarea { width: 100%; padding: 8px 10px; border: 1px solid var(--line2); border-radius: 7px; background: var(--field); color: var(--text); font: 12px var(--mono); resize: vertical; outline: none; margin-bottom: 6px; }
  .mirror { color: var(--text-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; margin-bottom: 4px; }
  .good { color: var(--up); }
  .bad { color: var(--down); }
  .segmap span { background: var(--line2); border-radius: 2px; overflow: hidden; min-width: 2px; }
  .segmap i { display: block; height: 100%; background: var(--bar); transition: width 400ms linear; }
</style>
