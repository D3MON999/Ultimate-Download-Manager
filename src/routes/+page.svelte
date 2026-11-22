<script lang="ts">
  import "$lib/theme/obsidian.css";
  import "$lib/theme/dialogs.css";
  import { applyTheme } from "$lib/theme/apply";
  import { version as appVersion } from "../../package.json";
  import Icon from "$lib/Icon.svelte";
  import Logo from "$lib/Logo.svelte";
  import DetailsTabs from "$lib/DetailsTabs.svelte";
  import SplitView from "$lib/SplitView.svelte";
  import SettingsDialog from "$lib/SettingsDialog.svelte";
  import ErrorBox from "$lib/ErrorBox.svelte";
  import { friendly as friendlyError } from "$lib/errors";
  import NewDownload from "$lib/NewDownload.svelte";
  import Progress from "$lib/Progress.svelte";
  import VideoWindow from "$lib/VideoWindow.svelte";

  /// This window is a "Download file info" window (`?new=<token>`), not the main one.
  const newToken: string | null =
    typeof window === "undefined"
      ? null
      : ((window as unknown as { __UDM_NEW__?: string }).__UDM_NEW__ ?? new URLSearchParams(location.search).get("new"));
  /// … or the video button's window (`?video=<page>`).
  const videoAsk: { page: string; title: string } | null = (() => {
    if (typeof window === "undefined") return null;
    const g = (window as unknown as { __UDM_VIDEO__?: { page: string; title: string } }).__UDM_VIDEO__;
    if (g) return g;
    const q = new URLSearchParams(location.search).get("video");
    return q ? { page: q, title: "" } : null;
  })();
  /// … or a download's progress window (`?progress=<id>`).
  const progressId: number | null = (() => {
    if (typeof window === "undefined") return null;
    const v = (window as unknown as { __UDM_PROGRESS__?: number }).__UDM_PROGRESS__ ?? new URLSearchParams(location.search).get("progress");
    return v == null ? null : Number(v);
  })();
  import VideoDialog from "$lib/VideoDialog.svelte";
  import TorrentDialog from "$lib/TorrentDialog.svelte";
  import GrabberDialog from "$lib/GrabberDialog.svelte";
  import SubsDialog from "$lib/SubsDialog.svelte";
  import type { IconName } from "$lib/icons";
  import { api, ASSIST_NEEDED, CHECK_NEEDED, inTauri, onChanged, onClipboard, onUpdate, type RemoteInfo, onProgress, onSession, onShow, onToast, type LinkInfo, type Row, type Settings, type Tick } from "$lib/api";
  import * as fmt from "$lib/format";
  import { open as pickFolder, save as pickSave } from "@tauri-apps/plugin-dialog";
  import { onMount, tick } from "svelte";

  type Cat = { id: string; label: string; icon: IconName; match: (r: Row) => boolean };

  const categories: Cat[] = [
    { id: "all", label: "All downloads", icon: "all", match: () => true },
    { id: "active", label: "Downloading", icon: "active", match: (r) => r.state === "downloading" },
    { id: "queued", label: "Queued", icon: "queue", match: (r) => r.state === "queued" },
    { id: "stopped", label: "Paused & failed", icon: "stopped", match: (r) => r.state === "paused" || r.state === "failed" },
    { id: "done", label: "Finished", icon: "done", match: (r) => r.state === "done" },
    { id: "video", label: "Video", icon: "video", match: (r) => r.category === "video" },
    { id: "music", label: "Music", icon: "music", match: (r) => r.category === "music" },
    { id: "docs", label: "Documents", icon: "doc", match: (r) => r.category === "docs" },
    { id: "archives", label: "Archives", icon: "archive", match: (r) => r.category === "archives" },
    { id: "programs", label: "Programs", icon: "app", match: (r) => r.category === "programs" },
    { id: "images", label: "Images", icon: "image", match: (r) => r.category === "images" },
    { id: "torrents", label: "Torrents", icon: "magnet", match: (r) => r.category === "torrents" },
  ];

  const catIcon: Record<string, IconName> = {
    video: "video", music: "music", docs: "doc", archives: "archive", programs: "app", images: "image", torrents: "magnet",
  };


  // Video dialog
  let videoOpen = $state(false);
  let videoUrl = $state("");
  let videoDir = $state("");

  async function openVideo(url: string) {
    paletteOpen = false;
    addOpen = false;
    videoUrl = url;
    if (inTauri) videoDir = (await api.settings()).download_dir;
    videoOpen = false;
    await tick();
    videoOpen = true;
  }

  let subsOpen = $state(false);

  // Link grabber
  let grabOpen = $state(false);
  let grabText = $state("");
  let grabDir = $state("");
  let copied = $state<string[]>([]);

  async function openGrabber(text: string) {
    paletteOpen = false;
    addOpen = false;
    copied = [];
    grabText = text;
    if (inTauri) grabDir = (await api.settings()).download_dir;
    grabOpen = false;
    await tick();
    grabOpen = true;
  }

  // Torrent dialog
  let torrentOpen = $state(false);
  let torrentSrc = $state("");
  let torrentDir = $state("");

  async function openTorrent(src: string) {
    paletteOpen = false;
    addOpen = false;
    torrentSrc = src;
    if (inTauri) torrentDir = (await api.settings()).download_dir;
    torrentOpen = false;
    await tick();
    torrentOpen = true;
  }



  let updateFound = $state<{ version: string; notes: string | null } | null>(null);


  async function installUpdate() {
    say("Downloading the update… Ultimate Download Manager restarts by itself.");
    try {
      await api.installUpdate();
    } catch (e) {
      say(String(e), true);
    }
  }




  // Site check dialog
  let checkOpen = $state(false);
  let checkUrl = $state("");
  let checkInput = $state<HTMLInputElement | null>(null);

  async function openCheck(url: string) {
    paletteOpen = false;
    checkUrl = url;
    checkOpen = true;
    await tick();
    checkInput?.focus();
  }

  async function startCheck() {
    if (!checkUrl.trim()) return;
    try {
      await api.siteCheck(checkUrl.trim());
      checkOpen = false;
      say("Pass the check in the new window. Ultimate Download Manager continues by itself once it has passed.");
    } catch (e) {
      say(String(e), true);
    }
  }

  // Browser extension dialog
  let extOpen = $state(false);
  let ext = $state<{ chromium: string | null; firefox: string | null; host: { Ok?: string; Err?: string } } | null>(null);

  async function openExtension() {
    paletteOpen = false;
    extOpen = true;
    ext = null;
    try {
      ext = await api.extensionInfo();
    } catch (e) {
      say(String(e), true);
    }
  }

  const needsCheck = (r: Row) => r.state === "failed" && (r.error ?? "").startsWith(CHECK_NEEDED);
  const needsYou = (r: Row) => r.state === "failed" && (r.error ?? "").startsWith(ASSIST_NEEDED);

  let rows = $state<Row[]>([]);
  let active = $state("all");
  let selected = $state<Set<number>>(new Set());
  let toast = $state("");
  let toastBad = $state(false);
  let version = $state("");
  let built = $state(0);

  // Add dialog
  let addOpen = $state(false);
  let url = $state("");
  let info = $state<LinkInfo | null>(null);
  let probing = $state(false);
  let probeError = $state("");
  let name = $state("");
  let dir = $state("");
  let adding = $state(false);
  let addInput = $state<HTMLInputElement | null>(null);

  // Remove dialog
  let removeIds = $state<number[]>([]);

  // Settings dialog
  let settingsOpen = $state(false);

  // Command palette
  let paletteOpen = $state(false);
  let query = $state("");
  let paletteInput = $state<HTMLInputElement | null>(null);

  const downloading = $derived(rows.filter((r) => r.state === "downloading"));
  const totalSpeed = $derived(downloading.reduce((a, r) => a + r.speed, 0));
  const queuedCount = $derived(rows.filter((r) => r.state === "queued").length);

  const actions = $derived([
    { label: "Add a link", key: "Ctrl+N", run: openAdd },
    { label: "Settings", key: "Ctrl+,", run: openSettings },
    { label: "Site check (pass a browser check)", key: "", run: () => openCheck("") },
    { label: "Download a video or song from a page", key: "", run: () => openVideo("") },
    { label: "Open a magnet link or .torrent file", key: "", run: () => openTorrent("") },
    { label: "Link grabber (paste many links)", key: "", run: () => openGrabber("") },
    { label: "Download an image gallery (Instagram, Pixiv, Reddit…)", key: "", run: () => { paletteOpen = false; galleryOpen = true; } },
    { label: "Statistics: what I've downloaded", key: "", run: openStats },
    { label: "Clear the list…", key: "", run: () => { paletteOpen = false; clearOpen = true; } },
    { label: "Clear finished downloads from the list", key: "", run: () => run(api.clearList("finished", false).then((n) => say(`${n} cleared`))) },
    { label: "Batch download (numbered links like file[1-20].zip)", key: "", run: () => openGrabber("https://example.com/file[1-10].zip") },
    { label: "Subscriptions (channels, playlists)", key: "", run: () => { paletteOpen = false; subsOpen = true; } },
    { label: "Set up the browser extension", key: "", run: openExtension },
    { label: "Pause all", key: "", run: () => run(api.pause(rows.filter((r) => r.state === "downloading" || r.state === "queued").map((r) => r.id))) },
    { label: "Resume all", key: "", run: () => run(api.resume(rows.filter((r) => r.state === "paused" || r.state === "failed").map((r) => r.id))) },
    ...categories.map((c) => ({ label: `Show ${c.label.toLowerCase()}`, key: "", run: () => (active = c.id) })),
  ]);
  const matches = $derived(actions.filter((a) => a.label.toLowerCase().includes(query.trim().toLowerCase())).slice(0, 8));

  function say(text: string, bad = false) {
    toast = text;
    toastBad = bad;
    setTimeout(() => {
      if (toast === text) toast = "";
    }, 4000);
  }

  async function run(p: Promise<unknown>) {
    try {
      await p;
    } catch (e) {
      say(String(e), true);
    }
  }

  async function reload() {
    if (!inTauri) return;
    try {
      rows = await api.list();
      const ids = new Set(rows.map((r) => r.id));
      selected = new Set([...selected].filter((id) => ids.has(id)));
    } catch (e) {
      say(String(e), true);
    }
  }

  function applyTicks(ticks: Tick[]) {
    const byId = new Map(ticks.map((t) => [t.id, t]));
    rows = rows.map((r) => {
      const t = byId.get(r.id);
      return t ? { ...r, done: t.done, size: t.size ?? r.size, speed: t.speed, eta: t.eta, conns: t.conns } : r.state === "downloading" ? r : { ...r, speed: 0 };
    });
  }

  // ---- add
  let probeTimer: ReturnType<typeof setTimeout> | undefined;
  async function openAdd() {
    paletteOpen = false;
    addOpen = true;
    url = "";
    info = null;
    probeError = "";
    name = "";
    if (inTauri && !dir) dir = (await api.settings()).download_dir;
    await tick();
    addInput?.focus();
  }

  function onUrlInput() {
    info = null;
    probeError = "";
    clearTimeout(probeTimer);
    if (!url.trim()) return;
    probeTimer = setTimeout(probe, 450);
  }

  async function probe() {
    const asked = url.trim();
    if (!asked || !inTauri) return;
    probing = true;
    try {
      const got = await api.probe(asked);
      if (url.trim() !== asked) return;
      info = got;
      if (got.name && !name) name = got.name;
    } catch (e) {
      if (url.trim() === asked) probeError = String(e);
    } finally {
      probing = false;
    }
  }

  async function browse(current: string): Promise<string | null> {
    const picked = await pickFolder({ directory: true, defaultPath: current || undefined, title: "Save downloads in" });
    return typeof picked === "string" ? picked : null;
  }

  async function submitAdd(start: boolean) {
    if (!url.trim()) return;
    if (url.trim().toLowerCase().startsWith("magnet:?")) return openTorrent(url.trim());
    adding = true;
    try {
      const nameOrNull = name.trim() && name.trim() !== info?.name ? name.trim() : null;
      await api.add(url.trim(), dir || null, nameOrNull, start);
      addOpen = false;
      say(start ? "Added to the queue" : "Added, paused");
    } catch (e) {
      probeError = String(e);
    } finally {
      adding = false;
    }
  }

  // ---- settings
  function openSettings() {
    paletteOpen = false;
    settingsOpen = true;
  }


  let prefs = $state<Settings | null>(null);
  // ---- queues: Main and the person's own, in the side menu when there is more than Main
  const queueList = $derived([{ id: "main", name: "Main", paused: !!prefs?.main_paused }, ...(prefs?.queues ?? []).map((q) => ({ id: q.id, name: q.name, paused: q.paused }))]);
  const queueOf = (r: Row) => (r.queue && (prefs?.queues ?? []).some((q) => q.id === r.queue) ? r.queue : "main");
  const queueCats = $derived(
    queueList.length > 1
      ? queueList.map((q) => ({ id: `queue:${q.id}`, label: q.name, icon: "queue" as IconName, match: (r: Row) => queueOf(r) === q.id && r.state !== "done" }))
      : [],
  );
  const allCats = $derived([...categories, ...queueCats]);
  async function toggleQueue(id: string) {
    if (!prefs) return;
    const next = id === "main" ? { ...prefs, main_paused: !prefs.main_paused } : { ...prefs, queues: prefs.queues.map((q) => (q.id === id ? { ...q, paused: !q.paused } : q)) };
    try {
      prefs = (await api.saveSettings(next)) ?? next;
    } catch (e) {
      say(String(e), true);
    }
  }
  const visible = $derived(rows.filter((allCats.find((c) => c.id === active) ?? categories[0]).match));
  const counts = $derived(Object.fromEntries(allCats.map((c) => [c.id, rows.filter(c.match).length])));

  async function answerTorrents(yes: boolean) {
    if (!prefs) return;
    try {
      prefs = await api.saveSettings({ ...prefs, handle_torrents: yes, asked_torrents: true });
      if (yes) say("Magnet links and .torrent files now open here");
    } catch (e) {
      say(String(e), true);
    }
  }

  // ---- a long table draws only the rows on screen (fixed height rows)
  const ROW_H = 36;
  const WINDOW_FROM = 400;
  let tableWrap = $state<HTMLDivElement | null>(null);
  let tableTop = $state(0);
  let tableH = $state(800);
  const win = $derived.by(() => {
    if (shown.length <= WINDOW_FROM) return { rows: shown, before: 0, after: 0 };
    const first = Math.max(0, Math.floor(tableTop / ROW_H) - 20);
    const last = Math.min(shown.length, Math.ceil((tableTop + tableH) / ROW_H) + 20);
    return { rows: shown.slice(first, last), before: first * ROW_H, after: (shown.length - last) * ROW_H };
  });

  // ---- right-click menu, remove, clear list
  let menu = $state<{ x: number; y: number } | null>(null);
  let removeDelete = $state(false);
  const picked = $derived(rows.filter((r) => selected.has(r.id)));

  function openMenu(r: Row, e: MouseEvent) {
    e.preventDefault();
    if (!selected.has(r.id)) {
      selected = new Set([r.id]);
      anchor = r.id;
    }
    // Kept inside the window.
    const w = 240, h = 420;
    menu = { x: Math.min(e.clientX, innerWidth - w - 8), y: Math.min(e.clientY, innerHeight - h - 8) };
  }
  const closeMenu = () => (menu = null);
  function menuDo(f: () => unknown) {
    menu = null;
    f();
  }
  function askRemove(ids: number[], deleteFiles: boolean) {
    removeDelete = deleteFiles;
    removeIds = ids;
  }
  function copy(text: string, what: string) {
    navigator.clipboard?.writeText(text).then(() => say(`${what} copied`)).catch(() => say("Couldn't copy", true));
  }

  let clearOpen = $state(false);
  let clearWhich = $state<"finished" | "failed" | "missing" | "all">("finished");
  let clearDelete = $state(false);
  const clearCounts = $derived({
    finished: rows.filter((r) => r.state === "done").length,
    failed: rows.filter((r) => r.state === "failed").length,
    all: rows.length,
  });
  async function doClear() {
    try {
      const n = await api.clearList(clearWhich, clearDelete && clearWhich !== "missing");
      clearOpen = false;
      say(`${n} download${n === 1 ? "" : "s"} cleared${clearDelete && clearWhich !== "missing" ? " and their files deleted" : ""}`);
    } catch (e) {
      say(String(e), true);
    }
  }

  // ---- torrents: port check, making a .torrent

  // ---- settings: sections and helpers







  // ---- layout: the list-and-detail one, or the table (the earlier one), switched at once
  let layoutPref = $state<"split" | "table" | null>(null);
  const layout = $derived(layoutPref ?? prefs?.layout ?? "split");
  async function switchLayout() {
    const next = layout === "split" ? "table" : "split";
    layoutPref = next;
    if (!inTauri || !prefs) return;
    try {
      const saved = await api.saveSettings({ ...prefs, layout: next });
      prefs = saved ?? { ...prefs, layout: next };
    } catch (e) {
      say(String(e), true);
    }
  }

  // ---- theme
  let dockOn = $state(false);
  // The Dock theme's names for the state categories (the rest keep theirs).
  const DOCK_LABEL: Record<string, string> = { all: "Everything", active: "Arriving", queued: "Up next", stopped: "Stopped", done: "Delivered" };
  $effect(() => {
    dockOn = applyTheme(prefs?.theme) === "dock";
  });

  // ---- galleries
  let galleryOpen = $state(false);
  let galleryUrl = $state("");
  let galleryLimit = $state(0);
  let galleryBusy = $state(false);
  async function addGallery() {
    if (!galleryUrl.trim()) return;
    galleryBusy = true;
    try {
      await api.addGallery(galleryUrl.trim(), null, Number(galleryLimit) || 0);
      galleryOpen = false;
      galleryUrl = "";
      say("Gallery added to the queue");
    } catch (e) {
      say(String(e), true);
    } finally {
      galleryBusy = false;
    }
  }

  // ---- stats
  let statsOpen = $state(false);
  let stats = $state<import("$lib/api").Stats | null>(null);
  async function openStats() {
    paletteOpen = false;
    statsOpen = true;
    stats = null;
    try {
      stats = await api.stats();
    } catch (e) {
      say(String(e), true);
    }
  }

  // ---- drag and drop: links, .torrent files
  let dropping = $state(false);
  function onDrop(e: DragEvent) {
    e.preventDefault();
    dropping = false;
    const text = e.dataTransfer?.getData("text/uri-list") || e.dataTransfer?.getData("text/plain") || "";
    if (text.trim()) {
      const links = text.split(/\s+/).filter((l) => /^(https?|ftp|magnet):/i.test(l));
      if (links.length === 1 && links[0].toLowerCase().startsWith("magnet:")) return openTorrent(links[0]);
      if (links.length) return openGrabber(links.join(String.fromCharCode(10)));
    }
  }

  // ---- the table: sorting, filtering, selection
  type SortKey = "name" | "size" | "progress" | "speed" | "eta" | "status" | "added";
  let sortKey = $state<SortKey>("added");
  let sortDesc = $state(true);
  let filter = $state("");
  let anchor = $state<number | null>(null);

  const STATE_ORDER: Record<string, number> = { downloading: 0, queued: 1, paused: 2, failed: 3, done: 4 };
  const sortValue = (r: Row, k: SortKey): number | string => {
    switch (k) {
      case "name": return r.display.toLowerCase();
      case "size": return r.size ?? -1;
      case "progress": return r.size ? r.done / r.size : r.state === "done" ? 1 : 0;
      case "speed": return r.speed;
      case "eta": return r.eta ?? Number.MAX_SAFE_INTEGER;
      case "status": return STATE_ORDER[r.state] ?? 9;
      case "added": return r.created_at;
    }
  };
  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    // `#tag` words match tags; the rest, the name or the address.
    const tagWords = q.split(/\s+/).filter((w) => w.startsWith("#") && w.length > 1).map((w) => w.slice(1));
    const text = q.split(/\s+/).filter((w) => !w.startsWith("#")).join(" ").trim();
    const list = q
      ? visible.filter((r) => {
          const tags = (r.tags ?? "").split(" ");
          return tagWords.every((t) => tags.some((x) => x.startsWith(t))) && (!text || r.display.toLowerCase().includes(text) || r.url.toLowerCase().includes(text));
        })
      : visible;
    return [...list].sort((a, b) => {
      const x = sortValue(a, sortKey), y = sortValue(b, sortKey);
      const c = x < y ? -1 : x > y ? 1 : 0;
      return sortDesc ? -c : c;
    });
  });
  const focused = $derived(selected.size === 1 ? rows.find((r) => selected.has(r.id)) ?? null : null);

  function sortBy(k: SortKey) {
    if (sortKey === k) sortDesc = !sortDesc;
    else {
      sortKey = k;
      sortDesc = k !== "name";
    }
  }

  function select(r: Row, e: MouseEvent) {
    if (e.shiftKey && anchor != null) {
      const ids = shown.map((x) => x.id);
      const [a, b] = [ids.indexOf(anchor), ids.indexOf(r.id)].sort((m, n) => m - n);
      selected = new Set(ids.slice(a, b + 1));
      return;
    }
    if (e.ctrlKey) {
      const s = new Set(selected);
      s.has(r.id) ? s.delete(r.id) : s.add(r.id);
      selected = s;
    } else {
      selected = new Set([r.id]);
    }
    anchor = r.id;
  }

  /// Arrow keys move the selection through the table.
  function moveSelection(step: number) {
    if (!shown.length) return;
    const ids = shown.map((x) => x.id);
    const at = anchor != null ? ids.indexOf(anchor) : -1;
    const next = ids[Math.max(0, Math.min(ids.length - 1, at + step))];
    selected = new Set([next]);
    anchor = next;
    const el = document.getElementById(`row-${next}`);
    if (el) el.scrollIntoView({ block: "nearest" });
    else if (tableWrap) {
      // A long table draws only what's on screen: scroll to the row by its place.
      const top = ids.indexOf(next) * ROW_H;
      if (top < tableWrap.scrollTop || top > tableWrap.scrollTop + tableWrap.clientHeight - ROW_H * 2) tableWrap.scrollTop = top - ROW_H * 3;
    }
  }

  function onSelected(act: (ids: number[]) => Promise<unknown>) {
    if (!selected.size) return say("Select downloads in the list first.");
    run(act([...selected]));
  }

  const statusLabel: Record<string, string> = { downloading: "Downloading", queued: "Queued", paused: "Paused", failed: "Failed", done: "Finished" };
  const linkWord = (r: Row) => (r.kind === "torrent" ? (r.conns === 1 ? "peer" : "peers") : r.conns === 1 ? "connection" : "connections");

  function toggle(r: Row) {
    if (r.state === "downloading" || r.state === "queued") run(api.pause([r.id]));
    else if (r.state === "paused" || r.state === "failed") run(api.resume([r.id]));
  }

  async function openPalette() {
    addOpen = false;
    paletteOpen = true;
    query = "";
    await tick();
    paletteInput?.focus();
  }

  function onKey(e: KeyboardEvent) {
    if (newToken || progressId != null || videoAsk) return;
    const typing = (e.target as HTMLElement)?.tagName === "INPUT";
    if (e.ctrlKey && e.key.toLowerCase() === "k") {
      e.preventDefault();
      openPalette();
    } else if (e.ctrlKey && e.key.toLowerCase() === "n") {
      e.preventDefault();
      openAdd();
    } else if (e.ctrlKey && e.key === ",") {
      e.preventDefault();
      openSettings();
    } else if (e.key === "Escape") {
      menu = null;
      clearOpen = false;
      addOpen = false;
      paletteOpen = false;
      settingsOpen = false;
      removeIds = [];
    } else if (!typing && e.key === "Delete" && selected.size) {
      askRemove([...selected], e.shiftKey);
    } else if (!typing && e.ctrlKey && e.key.toLowerCase() === "a") {
      e.preventDefault();
      selected = new Set(shown.map((r) => r.id));
    } else if (!typing && (e.key === "ArrowDown" || e.key === "ArrowUp") && !addOpen && !settingsOpen && !paletteOpen) {
      e.preventDefault();
      moveSelection(e.key === "ArrowDown" ? 1 : -1);
    } else if (!typing && (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) && selected.size) {
      e.preventDefault();
      const first = shown.find((r) => selected.has(r.id));
      const box = first ? document.getElementById(`row-${first.id}`)?.getBoundingClientRect() : null;
      menu = { x: Math.min((box?.left ?? 300) + 40, innerWidth - 248), y: Math.min((box?.bottom ?? 200) - 4, innerHeight - 428) };
    } else if (!typing && e.key === "Enter" && focused?.state === "done") {
      run(api.open(focused.id));
    } else if (!typing && e.key === " " && focused) {
      e.preventDefault();
      toggle(focused);
    }
  }

  function stateText(r: Row): string {
    switch (r.state) {
      case "downloading":
        return r.size
          ? `${fmt.speed(r.speed)} · ${fmt.eta(r.eta) || "estimating"} left · ${r.conns} ${r.kind === "torrent" ? (r.conns === 1 ? "peer" : "peers") : r.conns === 1 ? "connection" : "connections"}`
          : `${fmt.speed(r.speed)}`;
      case "queued":
        return "Queued";
      case "paused":
        return "Paused";
      case "failed":
        return r.error ?? "Failed";
      case "done":
        return `Finished ${fmt.when(r.finished_at)}`;
    }
  }

  /// Sample rows for previewing the layout in a plain browser (`?demo`), never inside the app.
  function demoRows(): Row[] {
    const now = Math.floor(Date.now() / 1000);
    const base = { referer: null, error: null, finished_at: null, speed: 0, eta: null, conns: 0, kind: "file", name: null, dir: "C:\\Users\\you\\Downloads" };
    const mk = (id: number, display: string, category: string, state: Row["state"], size: number | null, done: number, extra: Partial<Row> = {}): Row =>
      ({ ...base, id, url: `https://example.com/${encodeURIComponent(display)}`, file: `C:\\Users\\you\\Downloads\\${display}`, display, category, state, size, done, created_at: now - id * 600, ...extra }) as Row;
    return [
      mk(1, "ubuntu-24.04.1-desktop-amd64.iso", "programs", "downloading", 6_114_656_256, 2_402_000_000, { speed: 18_400_000, eta: 201, conns: 8 }),
      mk(2, "Big Buck Bunny (1080p).mp4", "video", "downloading", 725_000_000, 512_000_000, { speed: 6_200_000, eta: 34, conns: 1, kind: "video" }),
      mk(3, "debian-12.7.0-amd64-DVD-1.iso", "torrents", "downloading", 3_994_091_520, 911_000_000, { speed: 9_800_000, eta: 314, conns: 23, kind: "torrent" }),
      mk(4, "Annual report 2025.pdf", "docs", "queued", 14_300_000, 0),
      mk(5, "photos-backup.part1.rar", "archives", "paused", 2_147_483_648, 802_000_000, { queue: "night" }),
      mk(6, "setup-tool.exe", "programs", "failed", null, 0, { error: "Site check needed: the site wants a browser check first" }),
      mk(7, "podcast-episode-112.mp3", "music", "done", 88_400_000, 88_400_000, { finished_at: now - 3000 }),
      mk(8, "wallpapers.zip", "archives", "done", 312_000_000, 312_000_000, { finished_at: now - 86400 }),
      mk(9, "server-logs.tar.gz", "archives", "failed", 40_000_000, 12_000_000, { error: "The server closed the connection (HTTP 503) after 5 tries" }),
    ];
  }

  onMount(() => {
    if (newToken || progressId != null || videoAsk) return;
    if (!inTauri) {
      if (location.search.includes("demo")) {
        rows = demoRows();
        if (new URLSearchParams(location.search).has("many")) {
          const base = rows;
          rows = Array.from({ length: 3000 }, (_, i) => ({ ...base[i % base.length], id: i + 1, display: `${i + 1} · ${base[i % base.length].display}` }));
        }
        version = "preview";
        const q = new URLSearchParams(location.search);
        if (q.get("sel")) selected = new Set(q.get("sel")!.split(",").map(Number));
        if (q.has("menu")) menu = { x: 700, y: 140 };
        if (q.has("clear")) clearOpen = true;
        if (q.has("settings")) settingsOpen = true;
        if (q.get("theme") || q.get("layout") || q.has("queues"))
          prefs = {
            theme: q.get("theme") ?? "dock",
            layout: q.get("layout") ?? "split",
            queues: q.has("queues") ? [{ id: "night", name: "Tonight", parallel: 2, paused: true, from: "01:00", to: "07:00" }] : [],
          } as Settings;
      }
      return;
    }
    api.status().then((s) => {
      version = s.version;
      built = s.built;
    });
    api.settings().then((s) => (prefs = s));
    reload();
    const un = [
      onChanged(reload),
      onProgress(applyTicks),
      onToast((t) => say(t.text, t.bad)),
      onShow((what) => {
        if (what === "settings") return openSettings();
        try {
          const v = JSON.parse(what);
          if (v?.video) openVideo(v.video);
          if (v?.torrent) openTorrent(v.torrent);
          if (v?.grab) openGrabber(v.grab);
        } catch {}
      }),
      onClipboard((links) => (copied = links)),
      onUpdate((u) => (updateFound = u)),
      onSession((host) => say(`Site check passed for ${host}. Its downloads continue.`)),
    ];
    return () => un.forEach((p) => p.then((f) => f()));
  });
</script>

<svelte:window onkeydown={onKey} />

{#if newToken}
  <NewDownload token={newToken} />
{:else if progressId != null}
  <Progress id={progressId} />
{:else if videoAsk}
  <VideoWindow page={videoAsk.page} title={videoAsk.title} />
{:else}

<div class="shell" class:dropping class:split={layout === "split"} role="application"
  ondragover={(e) => { e.preventDefault(); dropping = true; }}
  ondragleave={(e) => { if (e.relatedTarget == null) dropping = false; }}
  ondrop={onDrop}>
  <header class="bar-top">
    <div class="brand" title="Ultimate Download Manager">
      <Logo size={24} />
      <span class="brand-name">Ultimate Download Manager</span>
    </div>

    <div class="tools" role="toolbar" aria-label="Downloads">
      <button class="tb primary" onclick={openAdd} title="Add a link (Ctrl+N)"><Icon name="plus" size={16} /><span>Add</span></button>
      <button class="tb" onclick={() => openVideo("")} title="Download a video or song from a page"><Icon name="video" size={16} /><span>Video</span></button>
      <button class="tb" onclick={() => openTorrent("")} title="Open a magnet link or .torrent file"><Icon name="magnet" size={16} /><span>Torrent</span></button>
      <button class="tb" onclick={() => openGrabber("")} title="Paste many links and sort them into packages"><Icon name="grab" size={16} /><span>Grabber</span></button>
      <button class="tb" onclick={() => (galleryOpen = true)} title="Download an image gallery (Instagram, Pixiv, Reddit, DeviantArt…)"><Icon name="image" size={16} /><span>Gallery</span></button>
      <span class="sep bulk" aria-hidden="true"></span>
      <button class="tb bulk" disabled={!selected.size} onclick={() => onSelected(api.resume)} title="Resume or retry the selected downloads"><Icon name="play" size={16} /><span>Resume</span></button>
      <button class="tb bulk" disabled={!selected.size} onclick={() => onSelected(api.pause)} title="Pause the selected downloads"><Icon name="pause" size={16} /><span>Pause</span></button>
      <button class="tb bulk" disabled={!selected.size} onclick={() => (removeIds = [...selected])} title="Remove the selected downloads (Delete)"><Icon name="trash" size={16} /><span>Remove</span></button>
      <span class="sep" aria-hidden="true"></span>
      <button class="tb" onclick={openSettings} title="Download hours, speed limits, what to do when done"><Icon name="clock" size={16} /><span>Scheduler</span></button>
      <button class="tb" onclick={() => openCheck("")} title="Pass a site's browser check once, then download fast"><Icon name="link" size={16} /><span>Site check</span></button>
    </div>

    <label class="filter">
      <Icon name="search" size={14} />
      <input bind:value={filter} placeholder="Filter downloads" spellcheck="false" autocomplete="off" aria-label="Filter downloads by name or address"
        onkeydown={(e) => e.key === "Escape" && (filter = "")} />
      {#if filter}<button class="clear" onclick={() => (filter = "")} aria-label="Clear the filter"><Icon name="close" size={12} /></button>{/if}
    </label>
    <button class="tb icon-only" onclick={switchLayout} title={layout === "split" ? "Switch to the table layout" : "Switch to the list-and-detail layout"} aria-label="Switch layout"><Icon name={layout === "split" ? "queue" : "layers"} size={16} /></button>
    <button class="tb icon-only" onclick={openPalette} title="All actions (Ctrl+K)" aria-label="All actions (Ctrl+K)"><kbd>Ctrl K</kbd></button>
  </header>

  <aside class="rail" aria-label="Categories">
    <div class="rail-group">
      {#each categories as c, i (c.id)}
        {#if i === 5}<div class="rail-label">{dockOn ? "Kinds" : "Type"}</div>{/if}
        <button class="cat" class:on={active === c.id} onclick={() => (active = c.id)} aria-current={active === c.id}>
          <Icon name={c.icon} size={16} />
          <span>{(dockOn ? DOCK_LABEL[c.id] ?? c.label : c.label)}</span>
          {#if counts[c.id]}<em>{counts[c.id]}</em>{/if}
        </button>
      {/each}
      {#if queueCats.length}
        <div class="rail-label">Queues</div>
        {#each queueList as q (q.id)}
          <div class="qrow">
            <button class="cat" class:on={active === `queue:${q.id}`} onclick={() => (active = `queue:${q.id}`)} aria-current={active === `queue:${q.id}`}>
              <Icon name="queue" size={16} />
              <span>{q.name}</span>
              {#if counts[`queue:${q.id}`]}<em>{counts[`queue:${q.id}`]}</em>{/if}
            </button>
            <button class="qtoggle" onclick={() => toggleQueue(q.id)} title={q.paused ? `Start ${q.name}` : `Stop ${q.name}`} aria-label={q.paused ? `Start ${q.name}` : `Stop ${q.name}`}><Icon name={q.paused ? "play" : "pause"} size={14} /></button>
          </div>
        {/each}
      {/if}
    </div>
    <div class="rail-group foot">
      <button class="cat" onclick={() => { clearWhich = clearCounts.finished ? "finished" : "failed"; clearDelete = false; clearOpen = true; }}><Icon name="trash" size={16} /><span>Clear list</span></button>
      <button class="cat" onclick={() => (subsOpen = true)}><Icon name="bell" size={16} /><span>Subscriptions</span></button>
      <button class="cat" onclick={openExtension}><Icon name="link" size={16} /><span>Browser extension</span></button>
      <button class="cat" onclick={openSettings}><Icon name="settings" size={16} /><span>Settings</span></button>
    </div>
  </aside>

  <main class="main">
    {#if layout === "split"}
      <SplitView
        rows={shown}
        all={rows}
        {selected}
        {focused}
        onselect={select}
        oncontext={openMenu}
        {needsCheck}
        {needsYou}
        filtering={!!filter.trim()}
        dock={dockOn}
        acts={{ run, say, toggle, remove: (ids) => (removeIds = ids), add: openAdd, extension: openExtension }} />
    {:else}
    <div class="table-wrap" bind:this={tableWrap} bind:clientHeight={tableH} onscroll={(e) => (tableTop = e.currentTarget.scrollTop)}>
      {#if shown.length === 0}
        <div class="empty">
          <Icon name="down" size={26} />
          {#if filter}
            <b>No downloads match “{filter}”</b>
            <span>Clear the filter or pick another category</span>
          {:else if rows.length}
            <b>Nothing in {allCats.find((c) => c.id === active)?.label.toLowerCase()}</b>
            <span>Pick another category on the left</span>
          {:else}
            <b>No downloads yet</b>
            <span>Press <kbd>Ctrl N</kbd> to add a link, or click a download in the browser once the extension is set up</span>
            <div class="empty-acts">
              <button class="btn primary" onclick={openAdd}><Icon name="plus" size={14} />Add a link</button>
              <button class="btn" onclick={openExtension}><Icon name="link" size={14} />Set up the browser extension</button>
            </div>
          {/if}
        </div>
      {:else}
        <table class="grid" role="grid" aria-label="Downloads" aria-multiselectable="true">
          <colgroup>
            <col class="c-name" /><col class="c-size" /><col class="c-prog" /><col class="c-speed" /><col class="c-eta" /><col class="c-status" /><col class="c-acts" />
          </colgroup>
          <thead>
            <tr>
              {#each [["name", "Name"], ["size", "Size"], ["progress", "Progress"], ["speed", "Speed"], ["eta", "Time left"], ["status", "Status"]] as [k, label] (k)}
                <th aria-sort={sortKey === k ? (sortDesc ? "descending" : "ascending") : "none"} class:num={k === "size" || k === "speed" || k === "eta"}>
                  <button onclick={() => sortBy(k as SortKey)}>
                    {label}{#if sortKey === k}<span class="arrow" aria-hidden="true">{sortDesc ? "▾" : "▴"}</span>{/if}
                  </button>
                </th>
              {/each}
              <th><span class="sr">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {#if win.before}<tr class="spacer" style="height:{win.before}px" aria-hidden="true"><td></td><td></td><td></td><td></td><td></td><td></td><td></td></tr>{/if}
            {#each win.rows as r (r.id)}
              {@const pct = fmt.percent(r.done, r.size)}
              <tr
                id="row-{r.id}"
                class="r {r.state}"
                class:sel={selected.has(r.id)}
                aria-selected={selected.has(r.id)}
                onclick={(e) => select(r, e)}
                oncontextmenu={(e) => openMenu(r, e)}
                ondblclick={() => (r.state === "done" ? run(api.open(r.id)) : run(api.showProgress(r.id)))}>
                <td class="name">
                  <span class="ficon"><Icon name={catIcon[r.category] ?? "file"} size={15} /></span>
                  <span class="nm" title={r.file ?? r.url}>{r.display}</span>
                </td>
                <td class="num mono">{r.state === "done" || !r.size ? fmt.bytes(r.size) : `${fmt.bytes(r.done)} / ${fmt.bytes(r.size)}`}</td>
                <td>
                  {#if r.state !== "done"}
                    <span class="pbar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={pct ?? undefined} aria-label="{r.display} progress">
                      <i style="width:{pct ?? (r.state === 'downloading' ? 100 : 0)}%" class:indet={pct == null && r.state === "downloading"}></i>
                    </span>
                    <span class="pct mono">{pct != null ? `${pct.toFixed(1)}%` : ""}</span>
                  {/if}
                </td>
                <td class="num mono">{r.state === "downloading" ? fmt.speed(r.speed) : ""}</td>
                <td class="num mono">{r.state === "downloading" ? fmt.eta(r.eta) || "…" : ""}</td>
                <td class="status">
                  <span class="chip {r.state}" title={r.state === "failed" ? friendlyError(r.error).title : ""}>
                    <i aria-hidden="true"></i>{needsYou(r) ? "Needs you" : needsCheck(r) ? "Site check" : (statusLabel[r.state])}
                  </span>
                </td>
                <td class="acts">
                  {#if r.state === "downloading" || r.state === "queued"}
                    <button onclick={(e) => { e.stopPropagation(); run(api.pause([r.id])); }} aria-label="Pause {r.display}" title="Pause"><Icon name="pause" size={14} /></button>
                  {:else if r.state === "paused" || r.state === "failed"}
                    <button onclick={(e) => { e.stopPropagation(); run(api.resume([r.id])); }} aria-label="{r.state === 'failed' ? 'Retry' : 'Resume'} {r.display}" title={r.state === "failed" ? "Retry" : "Resume"}><Icon name={r.state === "failed" ? "retry" : "play"} size={14} /></button>
                  {:else}
                    <button onclick={(e) => { e.stopPropagation(); run(api.open(r.id)); }} aria-label="Open {r.display}" title="Open"><Icon name="open" size={14} /></button>
                  {/if}
                  <button onclick={(e) => { e.stopPropagation(); run(api.reveal(r.id)); }} aria-label="Show {r.display} in folder" title="Show in folder"><Icon name="folder" size={14} /></button>
                </td>
              </tr>
            {/each}
            {#if win.after}<tr class="spacer" style="height:{win.after}px" aria-hidden="true"><td></td><td></td><td></td><td></td><td></td><td></td><td></td></tr>{/if}
          </tbody>
        </table>
      {/if}
    </div>

    {#if focused}
      {@const r = focused}
      {@const pct = fmt.percent(r.done, r.size)}
      <section class="drawer" aria-label="Details of {r.display}">
        <div class="drawer-head">
          <span class="ficon big"><Icon name={catIcon[r.category] ?? "file"} size={18} /></span>
          <div class="drawer-title">
            <b title={r.display}>{r.display}</b>
            <span class="chip {r.state}"><i aria-hidden="true"></i>{needsYou(r) ? "Needs you" : needsCheck(r) ? "Site check needed" : (statusLabel[r.state])}</span>
            {#if r.state === "downloading"}<span class="muted mono">{fmt.speed(r.speed)} · {r.conns} {linkWord(r)}</span>{/if}
          </div>
          <div class="drawer-acts">
            {#if r.state === "downloading" || r.state === "queued"}
              <button class="btn" onclick={() => run(api.pause([r.id]))}><Icon name="pause" size={14} />Pause</button>
            {:else if r.state === "paused" || r.state === "failed"}
              <button class="btn" onclick={() => run(api.resume([r.id]))}><Icon name={r.state === "failed" ? "retry" : "play"} size={14} />{r.state === "failed" ? "Retry" : "Resume"}</button>
            {:else}
              <button class="btn primary" onclick={() => run(api.open(r.id))}><Icon name="open" size={14} />Open</button>
            {/if}
            {#if r.state !== "done" && r.done > 0 && (r.category === "video" || r.category === "music" || r.kind === "torrent")}
              <button class="btn" onclick={() => run(api.preview(r.id))} title="Watch what has arrived so far in VLC"><Icon name="eye" size={14} />Preview</button>
            {/if}
            <button class="btn" onclick={() => run(api.reveal(r.id))}><Icon name="folder" size={14} />Folder</button>
            <button class="btn danger" onclick={() => (removeIds = [r.id])}><Icon name="trash" size={14} />Remove</button>
          </div>
        </div>

        {#if needsYou(r)}
          <div class="fix" role="alert">
            <span>This file host wants you to wait for its timer or solve a captcha. Do that in a browser window; Ultimate Download Manager takes the file from there with every connection.</span>
            <button class="btn primary" onclick={() => run(api.assist(r.id))}><Icon name="link" size={14} />Get it in a browser window</button>
          </div>
        {:else if needsCheck(r)}
          <div class="fix" role="alert">
            <span>This site wants a browser check before it lets the download through.</span>
            <button class="btn primary" onclick={() => { run(api.siteCheck(r.url)); say("Pass the check in the new window. Ultimate Download Manager continues by itself."); }}><Icon name="link" size={14} />Pass site check</button>
            <button class="btn" onclick={() => run(api.browserDownload(r.id))}><Icon name="down" size={14} />Download in a browser window</button>
          </div>
        {:else if r.state === "failed" && r.error}
          <ErrorBox row={r} onsay={say} onsettings={openSettings} />
        {/if}

        <div class="drawer-bar" aria-hidden="true">
          <i style="width:{r.state === 'done' ? 100 : (pct ?? 0)}%" class:indet={pct == null && r.state === "downloading"}></i>
        </div>

        <DetailsTabs row={r} onsay={say} />
      </section>
    {:else if selected.size > 1}
      <section class="drawer slim" aria-label="Selection">
        <b>{selected.size} downloads selected</b>
        <span class="muted">{fmt.bytes(rows.filter((r) => selected.has(r.id)).reduce((a, r) => a + (r.size ?? 0), 0))} in all</span>
        <div class="drawer-acts">
          <button class="btn" onclick={() => onSelected(api.resume)}><Icon name="play" size={14} />Resume</button>
          <button class="btn" onclick={() => onSelected(api.pause)}><Icon name="pause" size={14} />Pause</button>
          <button class="btn danger" onclick={() => (removeIds = [...selected])}><Icon name="trash" size={14} />Remove</button>
        </div>
      </section>
    {/if}
      {/if}
  </main>

  <footer class="statusbar" role="status" aria-live="polite" aria-atomic="true">
    <span><i class="dot" class:on={downloading.length > 0}></i>{downloading.length} downloading</span>
    <span>{queuedCount} queued</span>
    <span class="mono">↓ {fmt.speed(totalSpeed)}</span>
    {#if prefs?.global_limit_kib}<span>Limit {prefs.global_limit_kib} KiB/s</span>{/if}
    <span class="grow"></span>
    {#if prefs}
      <span title="Downloads only use this network connection">{prefs.adapter ? `Network: ${prefs.adapter}` : "Any network"}</span>
      <span>{prefs.schedule_enabled ? `Download hours ${prefs.schedule_start}–${prefs.schedule_stop}` : "No schedule"}</span>
    {/if}
    {#if updateFound}<button class="link" onclick={openSettings}>Update {updateFound.version} ready</button>{/if}
    <span class="muted" title="This build was made {built ? new Date(built * 1000).toLocaleString() : ''}">{inTauri ? `v${version}${built ? ` · ${new Date(built * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short" })} ${new Date(built * 1000).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}` : ""}` : `v${appVersion}`}</span>
  </footer>
</div>

{#if toast}
  <div class="toast" class:bad={toastBad} role="status">{toast}</div>
{/if}

{#if addOpen}
  <div class="scrim" onclick={() => (addOpen = false)} role="presentation"></div>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="add-title">
    <div class="dialog-head">
      <h2 id="add-title">Add a link</h2>
      <button class="x" onclick={() => (addOpen = false)} aria-label="Close"><Icon name="close" size={16} /></button>
    </div>
    <label for="url">Address</label>
    <input id="url" bind:this={addInput} bind:value={url} oninput={onUrlInput} placeholder="https://… or ftp://…"
      onkeydown={(e) => e.key === "Enter" && submitAdd(true)} spellcheck="false" autocomplete="off" />
    <p class="note" class:bad={!!probeError || (info && !info.supported)} class:ok={info?.supported} role="status">
      {#if probing}Looking at the link…
      {:else if probeError}{probeError}
      {:else if info}{info.supported ? `${info.kind} · ${fmt.bytes(info.size)}${info.host ? ` · ${info.host}` : ""}` : info.kind}
      {:else}Ultimate Download Manager checks the link before it downloads.{/if}
    </p>
    <label for="name">File name</label>
    <input id="name" bind:value={name} placeholder="The server's name" spellcheck="false" autocomplete="off" />
    <label for="dir">Save in</label>
    <div class="pick">
      <input id="dir" bind:value={dir} spellcheck="false" autocomplete="off" />
      <button class="ghost" onclick={async () => { const p = await browse(dir); if (p) dir = p; }}><Icon name="folder" size={15} />Browse</button>
    </div>
    <div class="row-btns">
      <button class="ghost left" onclick={() => openVideo(url.trim())} disabled={!url.trim()}><Icon name="video" size={15} />It's a video page</button>
      <button class="ghost" onclick={() => submitAdd(false)} disabled={adding || !url.trim()}>Add paused</button>
      <button class="primary" onclick={() => submitAdd(true)} disabled={adding || !url.trim()}>Download</button>
    </div>
  </div>
{/if}

{#if updateFound && !settingsOpen}
  <div class="clip upd" role="status">
    <Icon name="down" size={16} />
    <span>Ultimate Download Manager {updateFound.version} is available</span>
    <button class="primary sm" onclick={installUpdate}>Install</button>
    <button class="x" onclick={() => (updateFound = null)} aria-label="Later"><Icon name="close" size={14} /></button>
  </div>
{/if}

{#if copied.length}
  <div class="clip" role="status">
    <Icon name="link" size={16} />
    <span>{copied.length} link{copied.length === 1 ? "" : "s"} copied</span>
    <button class="primary sm" onclick={() => openGrabber(copied.join("\n"))}>Grab</button>
    <button class="x" onclick={() => (copied = [])} aria-label="Dismiss"><Icon name="close" size={14} /></button>
  </div>
{/if}

{#if subsOpen}
  <SubsDialog onclose={() => (subsOpen = false)} onsay={say} />
{/if}

{#if grabOpen}
  <GrabberDialog text={grabText} dir={grabDir} onclose={() => (grabOpen = false)} onsay={say} {browse} ontorrent={openTorrent} />
{/if}

{#if torrentOpen}
  <TorrentDialog source={torrentSrc} dir={torrentDir} onclose={() => (torrentOpen = false)} onsay={say} {browse} />
{/if}

{#if videoOpen}
  <VideoDialog url={videoUrl} dir={videoDir} onclose={() => (videoOpen = false)} onsay={say} {browse} />
{/if}

{#if checkOpen}
  <div class="scrim" onclick={() => (checkOpen = false)} role="presentation"></div>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="chk-title">
    <div class="dialog-head">
      <h2 id="chk-title">Site check</h2>
      <button class="x" onclick={() => (checkOpen = false)} aria-label="Close"><Icon name="close" size={16} /></button>
    </div>
    <p class="note">Some sites (often behind Cloudflare) only let a real browser through. Ultimate Download Manager opens the page in a browser window. Pass the check there once, or sign in. Ultimate Download Manager then reuses that session for every download from the site.</p>
    <label for="chk">Page address</label>
    <input id="chk" bind:this={checkInput} bind:value={checkUrl} placeholder="https://…" spellcheck="false" autocomplete="off" onkeydown={(e) => e.key === "Enter" && startCheck()} />
    <div class="row-btns">
      <button class="ghost" onclick={() => (checkOpen = false)}>Cancel</button>
      <button class="primary" onclick={startCheck} disabled={!checkUrl.trim()}>Open the page</button>
    </div>
  </div>
{/if}

{#if extOpen}
  <div class="scrim" onclick={() => (extOpen = false)} role="presentation"></div>
  <div class="dialog wide" role="dialog" aria-modal="true" aria-labelledby="ext-title">
    <div class="dialog-head">
      <h2 id="ext-title">Browser extension</h2>
      <button class="x" onclick={() => (extOpen = false)} aria-label="Close"><Icon name="close" size={16} /></button>
    </div>
    <p class="note">The extension hands your browser's downloads to Ultimate Download Manager, adds "Download with Ultimate Download Manager" to the right-click menu, and shows a button over videos.</p>
    {#if !ext}
      <p class="note">Checking…</p>
    {:else}
      <p class="note" class:ok={!!ext.host.Ok} class:bad={!!ext.host.Err}>
        {ext.host.Ok ? "Ultimate Download Manager's browser connection is registered with Chrome, Edge, Brave, Opera, Vivaldi and Firefox." : `The browser connection isn't registered: ${ext.host.Err}`}
      </p>
      <h3>Chrome, Edge, Brave, Opera, Vivaldi</h3>
      <ol>
        <li>Open <code>chrome://extensions</code> (Edge: <code>edge://extensions</code>).</li>
        <li>Turn on <b>Developer mode</b>.</li>
        <li>Click <b>Load unpacked</b> and choose the folder below.</li>
      </ol>
      {#if ext.chromium}<button class="ghost" onclick={() => run(api.openFolder(ext!.chromium!))}><Icon name="folder" size={15} />Open the extension folder</button>{:else}<p class="note bad">The extension folder is missing. Reinstall Ultimate Download Manager.</p>{/if}
      <h3>Firefox</h3>
      <ol>
        <li>Open <code>about:debugging#/runtime/this-firefox</code>.</li>
        <li>Click <b>Load Temporary Add-on</b> and choose <code>manifest.json</code> in the folder below.</li>
      </ol>
      <p class="note">Firefox removes temporary add-ons when it closes. To keep it installed, the extension has to be signed by Mozilla (free, needs a Mozilla account).</p>
      {#if ext.firefox}<button class="ghost" onclick={() => run(api.openFolder(ext!.firefox!))}><Icon name="folder" size={15} />Open the Firefox folder</button>{/if}
    {/if}
  </div>
{/if}

{#if removeIds.length}
  <div class="scrim" onclick={() => (removeIds = [])} role="presentation"></div>
  <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="rm-title">
    <div class="dialog-head">
      <h2 id="rm-title">Remove {removeIds.length === 1 ? "this download" : `${removeIds.length} downloads`}?</h2>
    </div>
    <label class="check"><input type="checkbox" bind:checked={removeDelete} /> Also delete the downloaded files (finished ones go to the Recycle Bin; unfinished parts are deleted)</label>
    <p class="note">{removeDelete ? "The downloads leave the list and their files are deleted." : "The downloads leave the list; your files stay where they are."}</p>
    <div class="row-btns">
      <button class="ghost" onclick={() => (removeIds = [])}>Cancel</button>
      <button class={removeDelete ? "ghost danger" : "primary"} onclick={() => { run(api.remove(removeIds, removeDelete)); removeIds = []; }}>{removeDelete ? "Remove and delete files" : "Remove from list"}</button>
    </div>
  </div>
{/if}

{#if settingsOpen}
  <SettingsDialog
    onclose={() => (settingsOpen = false)}
    onsaved={(s) => { prefs = s; layoutPref = null; }}
    {say}
    {browse}
    {updateFound}
    {installUpdate} />
{/if}

{#if menu}
  {@const one = picked.length === 1 ? picked[0] : null}
  {@const done = picked.filter((r) => r.state === "done")}
  {@const running = picked.filter((r) => r.state === "downloading" || r.state === "queued")}
  {@const stopped = picked.filter((r) => r.state === "paused" || r.state === "failed")}
  <div class="menu-scrim" onclick={closeMenu} oncontextmenu={(e) => { e.preventDefault(); closeMenu(); }} role="presentation"></div>
  <div class="ctx" role="menu" style="left:{menu.x}px; top:{menu.y}px">
    {#if one?.state === "done"}
      <button role="menuitem" class="strong" onclick={() => menuDo(() => run(api.open(one.id)))}><Icon name="open" size={14} />Open</button>
      <button role="menuitem" onclick={() => menuDo(() => run(api.openWith(one.id)))}><Icon name="app" size={14} />Open with…</button>
    {/if}
    {#if one}
      <button role="menuitem" onclick={() => menuDo(() => run(api.reveal(one.id)))}><Icon name="folder" size={14} />Show in folder</button>
    {/if}
    {#if running.length}
      <button role="menuitem" onclick={() => menuDo(() => run(api.pause(running.map((r) => r.id))))}><Icon name="pause" size={14} />Pause{running.length > 1 ? ` (${running.length})` : ""}</button>
    {/if}
    {#if stopped.length}
      <button role="menuitem" onclick={() => menuDo(() => run(api.resume(stopped.map((r) => r.id))))}><Icon name="play" size={14} />Resume{stopped.length > 1 ? ` (${stopped.length})` : ""}</button>
    {/if}
    {#if one && one.state !== "done"}
      <button role="menuitem" onclick={() => menuDo(() => run(api.showProgress(one.id)))}><Icon name="active" size={14} />Progress window</button>
    {/if}
    {#if picked.some((r) => r.state !== "downloading" && r.kind !== "torrent")}
      <button role="menuitem" onclick={() => menuDo(() => run(api.redownload(picked.filter((r) => r.state !== "downloading" && r.kind !== "torrent").map((r) => r.id)).then(() => say("Downloading again from the start"))))}><Icon name="retry" size={14} />Download again</button>
    {/if}
    {#if picked.some((r) => r.state !== "done")}
      <button role="menuitem" onclick={() => menuDo(() => run(api.moveInQueue(picked.filter((r) => r.state !== "done").map((r) => r.id), true)))}><Icon name="queue" size={14} />Move to the top</button>
      {#if queueList.length > 1}
        {#each queueList.filter((q) => !picked.every((r) => queueOf(r) === q.id)) as q (q.id)}
          <button role="menuitem" onclick={() => menuDo(() => run(api.setQueue(picked.filter((r) => r.state !== "done").map((r) => r.id), q.id).then(() => say(`Moved to ${q.name}`))))}><Icon name="queue" size={14} />Move to {q.name}</button>
        {/each}
      {/if}
    {/if}
    <hr />
    <button role="menuitem" onclick={() => menuDo(() => copy(picked.map((r) => r.url).join("\n"), picked.length > 1 ? "Links" : "Link"))}><Icon name="link" size={14} />Copy {picked.length > 1 ? "links" : "link"}</button>
    {#if one?.file}
      <button role="menuitem" onclick={() => menuDo(() => copy(one.file ?? "", "File path"))}><Icon name="doc" size={14} />Copy file path</button>
    {/if}
    <hr />
    {#if done.length || picked.some((r) => r.done > 0)}
      <button role="menuitem" class="danger" onclick={() => menuDo(() => askRemove(picked.map((r) => r.id), true))}><Icon name="trash" size={14} />Delete {picked.length > 1 ? `${picked.length} downloads` : "download"} and files…</button>
    {/if}
    <button role="menuitem" onclick={() => menuDo(() => askRemove(picked.map((r) => r.id), false))}><Icon name="close" size={14} />Remove from list{picked.length > 1 ? ` (${picked.length})` : ""}</button>
  </div>
{/if}

{#if clearOpen}
  <div class="scrim" onclick={() => (clearOpen = false)} role="presentation"></div>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="clr-title">
    <div class="dialog-head">
      <h2 id="clr-title">Clear the list</h2>
      <button class="x" onclick={() => (clearOpen = false)} aria-label="Close"><Icon name="close" size={16} /></button>
    </div>
    <div class="clr">
      <label class="check"><input type="radio" bind:group={clearWhich} value="finished" /> Finished downloads ({clearCounts.finished})</label>
      <label class="check"><input type="radio" bind:group={clearWhich} value="failed" /> Failed downloads ({clearCounts.failed})</label>
      <label class="check"><input type="radio" bind:group={clearWhich} value="missing" /> Finished downloads whose file was moved or deleted</label>
      <label class="check"><input type="radio" bind:group={clearWhich} value="all" /> Everything ({clearCounts.all}), running downloads too</label>
    </div>
    <label class="check"><input type="checkbox" bind:checked={clearDelete} disabled={clearWhich === "missing"} /> Also delete their downloaded files (finished ones go to the Recycle Bin)</label>
    <p class="note">{clearDelete && clearWhich !== "missing" ? "The downloads leave the list and their files are deleted." : "Only the list is cleared: the files stay where they are."}</p>
    <div class="row-btns">
      <button class="ghost" onclick={() => (clearOpen = false)}>Cancel</button>
      <button class={clearDelete && clearWhich !== "missing" ? "ghost danger" : "primary"} onclick={doClear}>{clearDelete && clearWhich !== "missing" ? "Clear and delete files" : "Clear"}</button>
    </div>
  </div>
{/if}

{#if prefs && inTauri && !prefs.asked_torrents && !newToken && progressId == null && !videoAsk}
  <div class="ask-bar" role="dialog" aria-label="Magnet links">
    <Icon name="magnet" size={18} />
    <span>Open magnet links and .torrent files with Ultimate Download Manager? You can change this later in Settings → Torrents.</span>
    <button class="ghost sm" onclick={() => answerTorrents(false)}>No, keep my current app</button>
    <button class="primary sm" onclick={() => answerTorrents(true)}>Yes</button>
  </div>
{/if}

{#if galleryOpen}
  <div class="scrim" onclick={() => (galleryOpen = false)} role="presentation"></div>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="gal-title">
    <div class="dialog-head">
      <h2 id="gal-title">Download an image gallery</h2>
      <button class="x" onclick={() => (galleryOpen = false)} aria-label="Close"><Icon name="close" size={16} /></button>
    </div>
    <label for="gurl">Gallery, profile or post address</label>
    <input id="gurl" bind:value={galleryUrl} placeholder="https://… (Instagram, Pixiv, Reddit, DeviantArt, Flickr, Imgur…)" spellcheck="false" autocomplete="off"
      onkeydown={(e) => e.key === "Enter" && addGallery()} />
    <label for="glim">At most this many files (0 = all)</label>
    <input id="glim" type="number" min="0" bind:value={galleryLimit} />
    <p class="note">Uses gallery-dl, installed the first time and checked against its author's signature. The files go in a folder of their own; starting it again later fetches only the new ones.</p>
    <div class="row-btns">
      <button class="ghost" onclick={() => (galleryOpen = false)}>Cancel</button>
      <button class="primary" onclick={addGallery} disabled={galleryBusy || !galleryUrl.trim()}>{galleryBusy ? "Adding…" : "Download"}</button>
    </div>
  </div>
{/if}

{#if statsOpen}
  <div class="scrim" onclick={() => (statsOpen = false)} role="presentation"></div>
  <div class="dialog wide" role="dialog" aria-modal="true" aria-labelledby="st-title">
    <div class="dialog-head">
      <h2 id="st-title">What you've downloaded</h2>
      <button class="x" onclick={() => (statsOpen = false)} aria-label="Close"><Icon name="close" size={16} /></button>
    </div>
    {#if !stats}
      <p class="note">Counting…</p>
    {:else}
      {@const most = Math.max(1, ...stats.months.map((m) => m.bytes))}
      <p class="stat-big"><b>{stats.files}</b> files · <b>{fmt.bytes(stats.bytes)}</b></p>
      <h3>By month</h3>
      {#each stats.months as m (m.month)}
        <div class="stat-row"><span class="mono">{m.month}</span><span class="stat-bar"><i style="width:{(m.bytes / most) * 100}%"></i></span><span class="mono">{fmt.bytes(m.bytes)}</span><span class="muted">{m.files}</span></div>
      {:else}
        <p class="note">Nothing finished yet.</p>
      {/each}
      <h3>By type</h3>
      {#each stats.types as t (t.type)}
        <div class="stat-row"><span>{t.type}</span><span class="stat-bar"><i style="width:{(t.bytes / Math.max(1, stats.bytes)) * 100}%"></i></span><span class="mono">{fmt.bytes(t.bytes)}</span><span class="muted">{t.files}</span></div>
      {/each}
    {/if}
  </div>
{/if}

{#if paletteOpen}
  <div class="scrim" onclick={() => (paletteOpen = false)} role="presentation"></div>
  <div class="cmd" role="dialog" aria-modal="true" aria-label="Command palette">
    <div class="cmd-in">
      <Icon name="search" size={16} />
      <input bind:this={paletteInput} bind:value={query} placeholder="Type an action…" aria-label="Action"
        onkeydown={(e) => { if (e.key === "Enter" && matches[0]) { paletteOpen = false; matches[0].run(); } }} />
    </div>
    <ul>
      {#each matches as m (m.label)}
        <li><button onclick={() => { paletteOpen = false; m.run(); }}><span>{m.label}</span>{#if m.key}<kbd>{m.key}</kbd>{/if}</button></li>
      {:else}
        <li class="none">No matching action</li>
      {/each}
    </ul>
  </div>
{/if}
{/if}

<style>
  .shell { display: grid; grid-template-columns: 220px 1fr; grid-template-rows: 48px 1fr 28px; height: 100vh; }
  .mono { font-family: var(--mono); font-variant-numeric: tabular-nums; }
  .muted { color: var(--muted); }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
  kbd { font: 11px var(--mono); color: var(--muted); border: 1px solid var(--line2); border-radius: 5px; padding: 1px 5px; }
  .dot { display: inline-block; width: 7px; height: 7px; border-radius: 50%; background: var(--line3); }
  .dot.on { background: var(--up); box-shadow: 0 0 0 3px var(--up-soft); }

  /* toolbar */
  .bar-top { grid-column: 1 / -1; display: flex; align-items: center; gap: var(--s3); padding: 0 var(--s3) 0 var(--s4); border-bottom: 1px solid var(--line); background: var(--card); min-width: 0; }
  .brand { display: flex; align-items: center; gap: 9px; font-weight: 600; font-size: 13px; width: 204px; flex: none; white-space: nowrap; overflow: hidden; }
  .brand-name { overflow: hidden; text-overflow: ellipsis; }
  .tools { display: flex; align-items: center; gap: 2px; flex: 1; min-width: 0; overflow: hidden; }
  .tb { display: inline-flex; align-items: center; gap: 6px; height: 30px; padding: 0 10px; border: 1px solid transparent; border-radius: 7px; background: transparent; color: var(--text-2); font-size: 12.5px; white-space: nowrap; transition: background var(--t-fast) var(--ease), color var(--t-fast) var(--ease); }
  .tb:hover:not(:disabled) { background: var(--card-hover); color: var(--text); }
  .tb.primary { background: var(--hero); color: var(--hero-text); border-color: var(--hero); height: 30px; padding: 0 12px; font-weight: 600; }
  .tb.primary:hover { background: var(--hero); color: var(--hero-text); filter: brightness(0.92); }
  .tb:disabled { opacity: 0.35; }
  .tb.icon-only { padding: 0 6px; flex: none; }
  .sep { width: 1px; height: 18px; background: var(--line2); margin: 0 6px; flex: none; }
  .filter { display: flex; align-items: center; gap: 7px; width: 220px; flex: none; height: 30px; padding: 0 8px 0 10px; margin: 0; border: 1px solid var(--line2); border-radius: 7px; background: var(--field); color: var(--muted); }
  .filter:focus-within { border-color: var(--line3); }
  .filter input { flex: 1; min-width: 0; height: 100%; border: 0; outline: none; background: transparent; color: var(--text); font-size: 12.5px; }
  .filter .clear { border: 0; background: none; color: var(--muted); display: grid; place-items: center; padding: 2px; }

  /* category rail */
  .rail { display: flex; flex-direction: column; justify-content: space-between; padding: var(--s2) 6px; border-right: 1px solid var(--line); overflow: auto; min-height: 0; }
  .rail-group { display: flex; flex-direction: column; gap: 1px; }
  .rail-group.foot { padding-top: var(--s2); margin-top: var(--s2); border-top: 1px solid var(--line); }
  .rail-label { margin: 20px 10px 8px; font-size: 12px; color: var(--muted); }
  .qrow { display: flex; align-items: center; gap: 2px; }
  .qrow .cat { flex: 1; min-width: 0; }
  .qtoggle { flex: none; width: 28px; height: 28px; display: grid; place-items: center; border: 0; border-radius: 6px; background: transparent; color: var(--muted); }
  .qtoggle:hover { background: var(--card-hover); color: var(--text); }
  .cat { display: flex; align-items: center; gap: 9px; height: 30px; padding: 0 10px; border: 0; border-radius: 6px; background: transparent; color: var(--muted); text-align: left; font-size: 12.5px; transition: background var(--t-fast) var(--ease), color var(--t-fast) var(--ease); }
  .cat span { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cat em { font: normal 11px var(--mono); color: var(--muted); }
  .cat:hover { background: var(--card-hover); color: var(--text); }
  .cat.on { background: var(--card); color: var(--text); box-shadow: inset 2px 0 0 var(--text); }

  /* main: table + drawer */
  .main { display: flex; flex-direction: column; min-height: 0; min-width: 0; overflow: hidden; }
  .table-wrap { flex: 1; min-height: 0; overflow: auto; }
  .grid { width: 100%; border-collapse: separate; border-spacing: 0; table-layout: fixed; font-size: 12.5px; }
  .c-size { width: 150px; } .c-prog { width: 170px; } .c-speed { width: 96px; } .c-eta { width: 84px; } .c-status { width: 132px; } .c-acts { width: 80px; }
  .grid thead th { position: sticky; top: 0; z-index: 1; height: 32px; padding: 0; background: var(--bg); border-bottom: 1px solid var(--line2); text-align: left; font-weight: 500; }
  .grid th button { width: 100%; height: 32px; padding: 0 12px; border: 0; background: none; color: var(--muted); font-size: 11.5px; letter-spacing: 0.04em; text-align: inherit; display: flex; align-items: center; gap: 4px; }
  .grid th.num button { justify-content: flex-end; }
  .grid th button:hover { color: var(--text); }
  .grid th[aria-sort="ascending"] button, .grid th[aria-sort="descending"] button { color: var(--text); }
  .arrow { font-size: 10px; }
  .grid td { height: 36px; padding: 0 12px; border-bottom: 1px solid var(--line); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; color: var(--text-2); }
  .grid td.num { text-align: right; }
  .r { cursor: default; transition: background var(--t-fast) var(--ease); }
  .r:hover td { background: var(--card-hover); }
  .r.sel td { background: var(--sel); }
  .r.sel td:first-child { box-shadow: inset 2px 0 0 var(--text); }
  .name { display: flex; align-items: center; gap: 9px; }
  .grid td.name { display: table-cell; }
  .name .ficon { display: inline-grid; vertical-align: middle; margin-right: 9px; }
  .ficon { width: 24px; height: 24px; border-radius: 6px; border: 1px solid var(--line2); place-items: center; color: var(--text-2); flex: none; display: grid; }
  .ficon.big { width: 36px; height: 36px; border-radius: 9px; }
  .nm { color: var(--text); font-weight: 500; vertical-align: middle; }
  .pbar { display: inline-block; vertical-align: middle; width: calc(100% - 52px); height: 5px; border-radius: 3px; background: var(--line2); overflow: hidden; }
  .pbar i, .drawer-bar i { display: block; height: 100%; background: var(--bar); border-radius: 3px; transition: width 400ms linear; }
  .pbar.full i { background: var(--line3); }
  .r.paused .pbar i, .r.queued .pbar i { background: var(--muted); }
  .r.failed .pbar i { background: var(--down); }
  .pbar i.indet, .drawer-bar i.indet { opacity: 0.45; animation: pulse 1.2s ease-in-out infinite; }
  .pct { display: inline-block; width: 46px; text-align: right; vertical-align: middle; font-size: 11.5px; color: var(--muted); }
  .chip { display: inline-flex; align-items: center; gap: 6px; height: 20px; padding: 0; border: 0 !important; border-radius: 0; border-color: var(--line2); font-size: 11.5px; color: var(--text-2); }
  .chip i { width: 6px; height: 6px; border-radius: 50%; background: var(--muted); }
  .chip.downloading { color: var(--up); border-color: color-mix(in srgb, var(--up) 35%, transparent); }
  .chip.downloading i { background: var(--bar); }
  .chip.failed { color: var(--down); border-color: color-mix(in srgb, var(--down) 40%, transparent); }
  .chip.failed i { background: var(--down); }
  .chip.done i { background: var(--text-2); }
  .grid td.acts { text-align: right; padding: 0 8px; text-overflow: clip; }
  .acts button { width: 26px; height: 26px; display: inline-grid; place-items: center; border: 0; border-radius: 6px; background: transparent; color: var(--muted); opacity: 0; transition: opacity var(--t-fast) var(--ease); }
  .r:hover .acts button, .r.sel .acts button, .acts button:focus-visible { opacity: 1; }
  .acts button:hover { background: var(--line2); color: var(--text); }

  .empty { height: 100%; display: grid; place-items: center; align-content: center; gap: 8px; padding: var(--s6); color: var(--muted); text-align: center; }
  .empty b { color: var(--text); font-weight: 600; font-size: 15px; }
  .empty-acts { display: flex; gap: var(--s2); margin-top: var(--s2); }

  .btn { display: inline-flex; align-items: center; gap: 6px; height: 30px; padding: 0 12px; border-radius: 7px; border: 1px solid var(--line2); background: transparent; color: var(--text-2); font-size: 12.5px; font-weight: 500; white-space: nowrap; }
  .btn:hover { border-color: var(--line3); color: var(--text); }
  .btn.primary { height: 30px; padding: 0 12px; background: var(--hero); color: var(--hero-text); border-color: var(--hero); }
  .btn.danger { color: var(--down); border-color: color-mix(in srgb, var(--down) 35%, transparent); }

  .drawer { flex: none; border-top: 1px solid var(--line2); background: var(--card); padding: var(--s3) var(--s4); display: flex; flex-direction: column; gap: var(--s3); height: clamp(260px, 44vh, 520px); overflow: hidden; animation: fade var(--t-fast) var(--ease); }
  .drawer.slim { height: auto; }
  .drawer-title .muted { font-size: 12px; white-space: nowrap; }
  .drawer.slim { flex-direction: row; align-items: center; gap: var(--s3); }
  .drawer-head { display: flex; align-items: center; gap: var(--s3); min-width: 0; }
  .drawer-title { flex: 1; min-width: 0; display: flex; align-items: center; gap: var(--s2); }
  .drawer-title b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 14px; }
  .drawer-acts { display: flex; gap: 6px; flex: none; margin-left: auto; }
  .drawer-bar { height: 4px; border-radius: 3px; background: var(--line2); overflow: hidden; }
  .fix { display: flex; align-items: center; gap: var(--s2); flex-wrap: wrap; padding: 8px 10px; border: 1px solid var(--line2); border-radius: 8px; font-size: 12.5px; color: var(--text-2); }
  .fix span { flex: 1; min-width: 200px; }
  .fix.bad { color: var(--down); border-color: color-mix(in srgb, var(--down) 35%, transparent); user-select: text; }

  /* status bar */
  .shell.dropping::after { content: "Drop links or a .torrent file to add them"; position: fixed; inset: 12px; display: grid; place-items: center; border: 2px dashed var(--text-2); border-radius: 16px; background: color-mix(in srgb, var(--bg) 80%, transparent); color: var(--text); font-size: 16px; pointer-events: none; z-index: 50; }
  /* The list-and-detail layout is larger all round (the table keeps its compact sizes). */
  .shell.split { grid-template-columns: 240px 1fr; grid-template-rows: 60px 1fr 32px; }
  .shell.split .bar-top { gap: var(--s4); padding: 0 var(--s4) 0 var(--s5); }
  .shell.split .brand { width: 212px; font-size: 14.5px; gap: 11px; }
  .shell.split .tb { height: 36px; padding: 0 12px; font-size: 14px; border-radius: 8px; gap: 8px; }
  .shell.split .tb.primary { height: 36px; padding: 0 16px; font-weight: 500; }
  .shell.split .tb.icon-only { padding: 0 8px; }
  .shell.split .filter { height: 36px; width: 260px; font-size: 14px; border-radius: 8px; }
  .shell.split .filter input { font-size: 14px; }
  .shell.split .rail { padding: 12px 8px; }
  .shell.split .cat { height: 36px; font-size: 14px; gap: 12px; padding: 0 12px; border-radius: 8px; }
  .shell.split .cat em { font-size: 12px; }
  .shell.split .rail-label { font-size: 12px; margin: 24px 12px 8px; }
  .shell.split .statusbar { font-size: 12.5px; }
  .shell.split .bulk { display: none; }
  .shell.split .brand { width: auto; min-width: 0; max-width: 240px; font-size: 14px; font-weight: 500; }
  @media (min-width: 1200px) { .shell.split .tb:not(.primary) span { display: inline; } }
  .menu-scrim { position: fixed; inset: 0; z-index: 60; }
  .ctx { position: fixed; z-index: 61; min-width: 240px; padding: 5px; border-radius: 12px; background: var(--card); border: 1px solid var(--line2); box-shadow: 0 16px 40px rgba(0, 0, 0, 0.45); animation: fade var(--t-fast) var(--ease); }
  .ctx button { width: 100%; display: flex; align-items: center; gap: 10px; height: 34px; padding: 0 10px; border: 0; border-radius: 8px; background: none; color: var(--text-2); font-size: 13.5px; text-align: left; }
  .ctx button:hover, .ctx button:focus-visible { background: var(--card-hover); color: var(--text); }
  .ctx button.strong { color: var(--text); font-weight: 600; }
  .ctx button.danger { color: var(--down); }
  .ctx hr { border: 0; border-top: 1px solid var(--line); margin: 4px 2px; }
  .clr { display: flex; flex-direction: column; gap: 6px; margin: 4px 0 8px; }
  input[type="radio"] { width: 16px; height: 16px; margin: 0; accent-color: var(--text); }
  .ask-bar { position: fixed; left: 50%; bottom: 46px; transform: translateX(-50%); z-index: 40; display: flex; align-items: center; gap: 12px; max-width: min(760px, calc(100vw - 32px)); padding: 10px 12px 10px 16px; border: 1px solid var(--line3); border-radius: 12px; background: var(--card); box-shadow: 0 12px 34px rgba(0, 0, 0, 0.4); font-size: 13.5px; color: var(--text-2); }
  .ask-bar span { flex: 1; }
  .grid tr.spacer td { padding: 0; border: 0; height: inherit; }
  .statusbar { grid-column: 1 / -1; display: flex; align-items: center; gap: var(--s4); padding: 0 var(--s4); border-top: 1px solid var(--line); background: var(--card); color: var(--text-2); font-size: 11.5px; white-space: nowrap; overflow: hidden; }
  .statusbar span { display: inline-flex; align-items: center; gap: 6px; }
  .statusbar .grow { flex: 1; }
  .statusbar .link { border: 0; background: none; padding: 0; color: var(--up); font-size: 11.5px; text-decoration: underline; }

  .toast { position: fixed; bottom: 20px; left: 50%; transform: translateX(-50%); padding: 10px 16px; border-radius: 10px; background: var(--hero); color: var(--hero-text); font-weight: 500; box-shadow: 0 10px 40px rgba(0,0,0,.6); animation: rise var(--t-base) var(--ease); max-width: min(640px, 90vw); }
  .toast.bad { background: var(--down); color: #fff; }

  @keyframes fade { from { opacity: 0; } }
  @keyframes rise { from { opacity: 0; transform: translate(-50%, 8px); } }
  @keyframes pulse { 50% { opacity: 0.15; } }

  @media (max-width: 1480px) {
    .tb:not(.primary) span { display: none; }
    .c-eta, .grid th:nth-child(5), .grid td:nth-child(5) { display: none; }
  }
  @media (max-width: 900px) {
    .shell { grid-template-columns: 52px 1fr; }
    .brand { width: auto; }
    .brand-name, .cat span, .cat em, .rail-label { display: none; }
    .filter { width: 150px; }
    .c-size { width: 132px; }
    .c-prog { width: 130px; }
  }
</style>
