<script lang="ts">
  // Settings: the sections on the left, each one's choices on the right; saved together.
  import Icon from "$lib/Icon.svelte";
  import { api, inTauri, type RemoteInfo, type Settings } from "$lib/api";
  import { open as pickFolder, save as pickSave } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";

  let {
    onclose,
    onsaved,
    say,
    browse,
    updateFound,
    installUpdate,
  }: {
    onclose: () => void;
    onsaved: (s: Settings) => void;
    say: (text: string, bad?: boolean) => void;
    browse: (current: string) => Promise<string | null>;
    updateFound: { version: string; notes: string | null } | null;
    installUpdate: () => void;
  } = $props();

  // An update the check here found (the page's own, from the start, otherwise).
  let found = $state<{ version: string; notes: string | null } | null>(null);
  const update = $derived(found ?? updateFound);

  async function run(p: Promise<unknown>) {
    try {
      await p;
    } catch (e) {
      say(String(e), true);
    }
  }

  // Tools (yt-dlp, FFmpeg) in Settings
  let toolVersions = $state<{ ytdlp: string | null; ffmpeg: string | null } | null>(null);

  let installing = $state(false);

  let adapters = $state<{ name: string; description: string | null; up: boolean; addresses: string[] }[]>([]);

  let accounts = $state<{ id: number; service: string; user: string; key_page: string }[]>([]);

  let accService = $state("Real-Debrid");

  let accKey = $state("");

  let accBusy = $state(false);

  const KEY_PAGES: Record<string, string> = {
    "Real-Debrid": "https://real-debrid.com/apitoken",
    AllDebrid: "https://alldebrid.com/apikeys",
    Premiumize: "https://www.premiumize.me/account",
  };

  async function addAccount() {
    accBusy = true;
    try {
      const a = await api.addAccount(accService, accKey);
      accounts = [...accounts, a];
      accKey = "";
      say(`${a.service} account added: ${a.user}`);
    } catch (e) {
      say(String(e), true);
    } finally {
      accBusy = false;
    }
  }

  let remote = $state<RemoteInfo | null>(null);

  let pairCode = $state("");

  let updateMsg = $state("");

  async function checkUpdate() {
    updateMsg = "Checking…";
    try {
      const u = await api.checkUpdate();
      found = u;
      updateMsg = u ? `Ultimate Download Manager ${u.version} is available.` : "Ultimate Download Manager is up to date.";
    } catch (e) {
      updateMsg = String(e);
    }
  }

  const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

  async function installTools() {
    installing = true;
    try {
      toolVersions = await api.installTools();
      say("yt-dlp and FFmpeg are up to date");
    } catch (e) {
      say(String(e), true);
    } finally {
      installing = false;
    }
  }

  let draft = $state<Settings | null>(null);

  async function saveSettings() {
    if (!draft) return;
    try {
      await api.saveSettings({
        ...draft,
        max_parallel: Number(draft.max_parallel),
        connections: Number(draft.connections),
        global_limit_kib: Number(draft.global_limit_kib),
        slow_limit_kib: Number(draft.slow_limit_kib),
        remote_port: Number(draft.remote_port),
        torrent_port: Number(draft.torrent_port),
        torrent_down_kib: Number(draft.torrent_down_kib),
        torrent_up_kib: Number(draft.torrent_up_kib),
        seed_ratio: Number(draft.seed_ratio),
        seed_minutes: Number(draft.seed_minutes),
        rss_minutes: Number(draft.rss_minutes),
      });
      onsaved(await api.settings());
      onclose();
      say("Settings saved");
    } catch (e) {
      say(String(e), true);
    }
  }

  let portBusy = $state(false);

  let portSaid = $state("");

  async function checkPort() {
    portBusy = true;
    portSaid = "";
    try {
      const r = await api.portCheck();
      portSaid = r.reachable ? `Yes: port ${r.port} is open, peers can connect.` : `No: port ${r.port} is closed. Turn on UPnP, or forward it in your router (or on your VPN).`;
    } catch (e) {
      portSaid = String(e);
    } finally {
      portBusy = false;
    }
  }

  let ctSource = $state("");

  let ctTrackers = $state("");

  let ctSeed = $state(true);

  let ctBusy = $state(false);

  let ctSaid = $state("");

  async function makeTorrent() {
    const base = ctSource.split(/[\\/]/).filter(Boolean).pop() ?? "shared";
    const out = await pickSave({ title: "Save the .torrent", defaultPath: `${base}.torrent`, filters: [{ name: "Torrent", extensions: ["torrent"] }] });
    if (!out) return;
    ctBusy = true;
    ctSaid = "";
    try {
      const magnet = await api.createTorrent(ctSource, ctTrackers, out, ctSeed);
      ctSaid = `Saved. Magnet: ${magnet}`;
      navigator.clipboard?.writeText(magnet).catch(() => {});
      say(ctSeed ? "Torrent made and shared; its magnet link is copied" : "Torrent made; its magnet link is copied");
    } catch (e) {
      ctSaid = String(e);
    } finally {
      ctBusy = false;
    }
  }

  const SET_TABS: [string, string][] = [
    ["general", "General"], ["after", "After download"], ["network", "Network"], ["schedule", "Schedule"],
    ["torrents", "Torrents & RSS"], ["rules", "Rules"], ["accounts", "Accounts"], ["remote", "Phone"], ["app", "App"],
  ];

  let setTab = $state("general");

  let feedPreview = $state<{ title: string; guid: string; match: boolean }[] | null>(null);

  const proxyKind = (p: string) => (!p.trim() ? "none" : p.trim().toLowerCase() === "system" ? "system" : p.startsWith("pac:") ? "pac" : "manual");

  const proxyFor = (kind: string, current: string) =>
    kind === "none" ? "" : kind === "system" ? "system" : kind === "pac" ? (current.startsWith("pac:") ? current : "pac:") : proxyKind(current) === "manual" ? current : "socks5://127.0.0.1:1080";

  async function pickFile(title: string, extensions: string[]): Promise<string | null> {
    const f = await pickFolder({ directory: false, multiple: false, title, filters: [{ name: title, extensions }] });
    return typeof f === "string" ? f : null;
  }

  // The same matching as the app's (all words, none of the exclusions; * wildcards).
  function titleMatches(title: string, filter: string, exclude: string) {
    const t = title.toLowerCase();
    const has = (w: string) => (/[*?]/.test(w) ? new RegExp(w.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".")).test(t) : t.includes(w));
    const words = (x: string) => x.toLowerCase().split(/\s+/).filter(Boolean);
    return words(filter).every(has) && !words(exclude).some(has);
  }

  async function previewFeedItems(url: string, filter: string, exclude: string) {
    if (!url.trim()) return say("Enter the feed's address first.", true);
    feedPreview = null;
    try {
      const items = await api.previewFeed(url.trim());
      feedPreview = items.map((i) => ({ title: i.title, guid: i.guid, match: titleMatches(i.title, filter, exclude) }));
    } catch (e) {
      say(String(e), true);
    }
  }

  async function doExport() {
    const path = await pickSave({ title: "Save a backup", defaultPath: "ultimate-download-manager-backup.json", filters: [{ name: "Backup", extensions: ["json"] }] });
    if (!path) return;
    try {
      const n = await api.exportBackup(path);
      say(`Backup saved: settings and ${n} unfinished download${n === 1 ? "" : "s"}`);
    } catch (e) {
      say(String(e), true);
    }
  }

  async function doImport() {
    const path = await pickFile("Backup", ["json"]);
    if (!path) return;
    try {
      const n = await api.importBackup(path);
      onsaved(await api.settings());
      onclose();
      say(`Restored: settings and ${n} download${n === 1 ? "" : "s"} (paused)`);
    } catch (e) {
      say(String(e), true);
    }
  }

  onMount(async () => {
    if (!inTauri) {
      // The browser preview: the defaults, to see the dialog.
      draft = {
        download_dir: "D:\\Downloads", max_parallel: 3, connections: 8, boost_connections: true, global_limit_kib: 0, notify: true, close_to_tray: true,
        seed_when_done: false, extract_archives: true, delete_archives_after_extract: false, scan_downloads: false, sort_by_type: true,
        watch_clipboard: false, adapter: "", schedule_enabled: false, schedule_start: "01:00", schedule_stop: "07:00",
        schedule_days: [true, true, true, true, true, true, true], when_done: "nothing", slow_enabled: false, slow_from: "09:00",
        slow_to: "18:00", slow_limit_kib: 1024, remote_on: false, remote_port: 8463, update_repo: "", check_updates: true,
        torrent_port: 0, torrent_dht: true, torrent_upnp: true, torrent_down_kib: 0, torrent_up_kib: 0, seed_ratio: 0, seed_minutes: 0,
        torrent_blocklist: "", feeds: [], rss_minutes: 30, proxy: "", reconnect_command: "", archive_passwords: [], rules: [],
        theme: "dock", autostart: false, sound: false, ask_where: true, ask_skip_types: [], last_dirs: {}, layout: "split",
        handle_torrents: false, txt_menu: true, hotkey: "CommandOrControl+Alt+D", complete_window: true, complete_min_mb: 50,
        asked_torrents: true, site_limits: [], queues: [{ id: "night", name: "Tonight", parallel: 2, paused: false, from: "01:00", to: "07:00" }], main_paused: false, auto_convert: [], forget_after_days: 0,
      } as Settings;
      return;
    }
    draft = await api.settings();
    api.toolsStatus().then((t) => (toolVersions = t));
    api.adapters().then((a) => (adapters = a));
    api.accounts().then((a) => (accounts = a));
    api.remoteInfo().then((r) => (remote = r));
  });
</script>

{#if draft}
  <div class="scrim" onclick={() => onclose()} role="presentation"></div>
  <div class="dialog settings" role="dialog" aria-modal="true" aria-labelledby="set-title">
    <div class="dialog-head">
      <h2 id="set-title">Settings</h2>
      <button class="x" onclick={() => onclose()} aria-label="Close"><Icon name="close" size={16} /></button>
    </div>
    <div class="set-body">
      <nav class="set-nav" aria-label="Settings sections">
        {#each SET_TABS as [id, label] (id)}
          <button class:on={setTab === id} aria-current={setTab === id} onclick={() => (setTab = id)}>{(label)}</button>
        {/each}
      </nav>
      <div class="set-pane">
        {#if setTab === "general"}
          <label for="sdir">Download folder</label>
          <div class="pick">
            <input id="sdir" bind:value={draft.download_dir} spellcheck="false" />
            <button class="ghost" onclick={async () => { const p = await browse(draft!.download_dir); if (p && draft) draft.download_dir = p; }}><Icon name="folder" size={15} />Browse</button>
          </div>
          <div class="grid2">
            <div><label for="par">Downloads at once</label><input id="par" type="number" min="1" max="16" bind:value={draft.max_parallel} /></div>
            <div><label for="con">Connections per download</label><input id="con" type="number" min="1" max="32" bind:value={draft.connections} /></div>
          </div>
          <label class="check"><input type="checkbox" bind:checked={draft.boost_connections} /> Add connections when a site slows each one down (up to 32)</label>
          <h3>Queues</h3>
          <p class="note">Main runs "Downloads at once" in the download hours. Each queue of yours runs its own number at once, in its own hours if you set them. Right-click downloads to move them</p>
          {#each draft.queues as q, i (q.id)}
            <div class="pick queue">
              <input bind:value={q.name} placeholder="Tonight" aria-label="Queue name" />
              <input class="narrow" type="number" min="1" max="16" bind:value={q.parallel} aria-label="At once" /><span class="muted">at once</span>
              <input class="time" type="time" bind:value={q.from} aria-label="From" /><span class="muted">to</span>
              <input class="time" type="time" bind:value={q.to} aria-label="To" />
              <button class="ghost sm danger" onclick={() => draft && (draft.queues = draft.queues.filter((_, k) => k !== i))} aria-label="Remove queue"><Icon name="trash" size={13} /></button>
            </div>
          {/each}
          <button class="ghost" onclick={() => draft && (draft.queues = [...draft.queues, { id: `q${Date.now().toString(36)}`, name: draft.queues.length ? `Queue ${draft.queues.length + 1}` : "Tonight", parallel: 2, paused: false, from: "", to: "" }])}><Icon name="plus" size={15} />Add a queue</button>
          <label for="lim">Speed limit for all downloads (KB/s, 0 = none)</label>
          <input id="lim" type="number" min="0" bind:value={draft.global_limit_kib} />
          <label class="check"><input type="checkbox" bind:checked={draft.notify} /> Notify me when a download finishes or fails</label>
          <label class="check"><input type="checkbox" bind:checked={draft.sound} /> Play a sound when a download finishes</label>
          <label class="check"><input type="checkbox" bind:checked={draft.sort_by_type} /> Sort downloads into folders by type (Video, Music, Documents…)</label>
          <label class="check"><input type="checkbox" bind:checked={draft.watch_clipboard} /> Offer links I copy anywhere</label>
          <div class="pick">
            <label class="check"><input type="checkbox" bind:checked={draft.complete_window} /> Show the Download complete window for downloads of at least</label>
            <input class="narrow" type="number" min="0" bind:value={draft.complete_min_mb} disabled={!draft.complete_window} aria-label="Size in MB" /><span class="muted">MB</span>
          </div>
          <div class="grid2">
            <div><label for="hk">Shortcut to download the link I copied, from any program</label><input id="hk" bind:value={draft.hotkey} placeholder="CommandOrControl+Alt+D (empty: off)" spellcheck="false" /></div>
            <div><label for="fg">Take finished downloads off the list after (days, 0 = never; files stay)</label><input id="fg" type="number" min="0" bind:value={draft.forget_after_days} /></div>
          </div>
          <h3>Downloads from the browser</h3>
          <label class="check"><input type="checkbox" bind:checked={draft.ask_where} /> Ask where to save each one (the Download file info window)</label>
          {#if draft.ask_where}
            <label for="skipt">Never ask for these file types (start them in their category's folder)</label>
            <input id="skipt" value={draft.ask_skip_types.join(", ")} placeholder="rar, zip, mp4" spellcheck="false"
              oninput={(e) => draft && (draft.ask_skip_types = e.currentTarget.value.split(/[\s,;]+/).map((x) => x.trim().replace(/^\./, "").toLowerCase()).filter(Boolean))} />
          {/if}
        {:else if setTab === "after"}
          <h3>Archives</h3>
          <label class="check"><input type="checkbox" bind:checked={draft.extract_archives} /> Unpack archives (zip, 7z, rar, tar) next to them</label>
          <label class="check"><input type="checkbox" bind:checked={draft.delete_archives_after_extract} disabled={!draft.extract_archives} /> Then move the archive to the Recycle Bin</label>
          <label for="pw">Passwords to try on protected archives (one per line, tried in order)</label>
          <textarea id="pw" rows="4" spellcheck="false" value={draft.archive_passwords.join("\n")}
            oninput={(e) => draft && (draft.archive_passwords = e.currentTarget.value.split("\n").map((x) => x.trim()).filter(Boolean))}></textarea>
          <p class="note">A download can also have its own password (its details → Info).</p>
          <h3>Safety</h3>
          <label class="check"><input type="checkbox" bind:checked={draft.scan_downloads} /> Scan each download with Microsoft Defender</label>
        {:else if setTab === "network"}
          <label for="adp">Download only through this network adapter (pick your VPN, e.g. ExpressVPN)</label>
          <select id="adp" bind:value={draft.adapter}>
            <option value="">Any (the normal connection)</option>
            {#each adapters as a (a.name)}<option value={a.name}>{a.description ? `${a.description} (${a.name})` : a.name}{a.up ? "" : " (off)"}</option>{/each}
          </select>
          <p class="note">{draft.adapter ? "Kill switch on: if this adapter goes off, downloads stop instead of using your normal connection. Torrents follow it from Ultimate Download Manager's next start." : "Connect your VPN first, then pick its adapter here."}</p>
          <label for="prx">Proxy</label>
          <select id="prx" value={proxyKind(draft.proxy)} onchange={(e) => draft && (draft.proxy = proxyFor(e.currentTarget.value, draft.proxy))}>
            <option value="none">No proxy</option>
            <option value="system">Use Windows' proxy settings</option>
            <option value="manual">HTTP or SOCKS proxy</option>
            <option value="pac">Automatic configuration script (PAC)</option>
          </select>
          {#if proxyKind(draft.proxy) === "manual"}
            <input bind:value={draft.proxy} placeholder="socks5://user:pass@127.0.0.1:1080 or http://proxy:8080" spellcheck="false" aria-label="Proxy address" />
            <p class="note">A SOCKS5 proxy also carries torrents (from the next start). HTTP proxies carry downloads only.</p>
          {:else if proxyKind(draft.proxy) === "pac"}
            <input value={draft.proxy.slice(4)} oninput={(e) => draft && (draft.proxy = `pac:${e.currentTarget.value}`)} placeholder="http://wpad/wpad.dat" spellcheck="false" aria-label="Script address" />
          {/if}
          <h3>Per-site speed limits</h3>
          <p class="note">Each download from these sites goes at most this fast (a download's own limit wins).</p>
          {#each draft.site_limits as sl, i (i)}
            <div class="pick">
              <input bind:value={sl.host} placeholder="example.com" spellcheck="false" aria-label="Site" />
              <input class="narrow" type="number" min="0" bind:value={sl.kib} aria-label="KB/s" /><span class="muted">KB/s</span>
              <button class="ghost sm danger" onclick={() => draft && (draft.site_limits = draft.site_limits.filter((_, k) => k !== i))} aria-label="Remove"><Icon name="trash" size={13} /></button>
            </div>
          {/each}
          <button class="ghost" onclick={() => draft && (draft.site_limits = [...draft.site_limits, { host: "", kib: 1024 }])}><Icon name="plus" size={15} />Add a site</button>
          <h3>Reconnect</h3>
          <label for="rc">When a file host says to wait (its limit for your address), run this command</label>
          <input id="rc" bind:value={draft.reconnect_command} placeholder={'"C:\\Program Files (x86)\\ExpressVPN\\services\\ExpressVPN.CLI.exe" connect smart'} spellcheck="false" />
          <p class="note">For example your VPN's command line to switch server. It runs at most once a minute; the download tries again after the host's wait.</p>
        {:else if setTab === "schedule"}
          <label class="check"><input type="checkbox" bind:checked={draft.schedule_enabled} /> Download only between these hours</label>
          {#if draft.schedule_enabled}
            <div class="grid2">
              <div><label for="s1">From</label><input id="s1" type="time" bind:value={draft.schedule_start} /></div>
              <div><label for="s2">Until</label><input id="s2" type="time" bind:value={draft.schedule_stop} /></div>
            </div>
            <div class="days">{#each DAYS as d, i (d)}<label class="day"><input type="checkbox" bind:checked={draft.schedule_days[i]} />{d}</label>{/each}</div>
            <label for="wd">When everything has finished</label>
            <select id="wd" bind:value={draft.when_done}><option value="nothing">Do nothing</option><option value="sleep">Put the computer to sleep</option><option value="shutdown">Shut the computer down</option></select>
          {/if}
          <label class="check"><input type="checkbox" bind:checked={draft.slow_enabled} /> Go slower during these hours (leave speed for others)</label>
          {#if draft.slow_enabled}
            <div class="grid3">
              <div><label for="w1">From</label><input id="w1" type="time" bind:value={draft.slow_from} /></div>
              <div><label for="w2">Until</label><input id="w2" type="time" bind:value={draft.slow_to} /></div>
              <div><label for="w3">Limit (KB/s)</label><input id="w3" type="number" min="16" bind:value={draft.slow_limit_kib} /></div>
            </div>
          {/if}
        {:else if setTab === "torrents"}
          <div class="grid2">
            <div><label for="td">Download limit, all torrents (KB/s, 0 = none)</label><input id="td" type="number" min="0" bind:value={draft.torrent_down_kib} /></div>
            <div><label for="tu">Upload limit, all torrents (KB/s, 0 = none)</label><input id="tu" type="number" min="0" bind:value={draft.torrent_up_kib} /></div>
          </div>
          <label class="check"><input type="checkbox" bind:checked={draft.handle_torrents} /> Open magnet links and .torrent files with Ultimate Download Manager</label>
          <h3>Sharing</h3>
          <label class="check"><input type="checkbox" bind:checked={draft.seed_when_done} /> Keep sharing torrents after they finish</label>
          {#if draft.seed_when_done}
            <div class="grid2">
              <div><label for="sr">Stop at ratio (0 = never)</label><input id="sr" type="number" min="0" step="0.1" bind:value={draft.seed_ratio} /></div>
              <div><label for="sm">…or after minutes (0 = never)</label><input id="sm" type="number" min="0" bind:value={draft.seed_minutes} /></div>
            </div>
          {/if}
          <h3>Connection <span class="muted small">(from the next start)</span></h3>
          <label for="tp">Listening port (0 = pick one at each start)</label>
          <input id="tp" type="number" min="0" max="65535" bind:value={draft.torrent_port} />
          <label class="check"><input type="checkbox" bind:checked={draft.torrent_dht} /> Find peers without trackers (DHT)</label>
          <label class="check"><input type="checkbox" bind:checked={draft.torrent_upnp} /> Ask the router to open the port (UPnP)</label>
          <label for="bl">IP blocklist file (P2P or eMule .dat, gzipped is fine)</label>
          <div class="pick">
            <input id="bl" bind:value={draft.torrent_blocklist} placeholder="None" spellcheck="false" />
            <button class="ghost" onclick={async () => { const f = await pickFile("Blocklist", ["p2p", "dat", "txt", "gz"]); if (f && draft) draft.torrent_blocklist = f; }}><Icon name="folder" size={15} />Browse</button>
          </div>
          <div class="pick">
            <button class="ghost" onclick={checkPort} disabled={portBusy}><Icon name="link" size={15} />{portBusy ? "Checking…" : "Can peers reach me?"}</button>
            {#if portSaid}<span class="note" class:ok={portSaid.startsWith("Yes")} class:bad={portSaid.startsWith("No")}>{portSaid}</span>{/if}
          </div>
          <h3>Make a .torrent</h3>
          <div class="pick">
            <input bind:value={ctSource} placeholder="The file or folder to share" spellcheck="false" aria-label="File or folder" />
            <button class="ghost" onclick={async () => { const f = await pickFile("File", ["*"]); if (f) ctSource = f; }}>File…</button>
            <button class="ghost" onclick={async () => { const f = await browse(""); if (f) ctSource = f; }}>Folder…</button>
          </div>
          <textarea rows="2" bind:value={ctTrackers} placeholder="Trackers, one per line (udp://tracker.opentrackr.org:1337/announce)" spellcheck="false"></textarea>
          <div class="pick">
            <label class="check"><input type="checkbox" bind:checked={ctSeed} /> Share it now</label>
            <button class="primary" disabled={!ctSource.trim() || ctBusy} onclick={makeTorrent}>{ctBusy ? "Making…" : "Make .torrent…"}</button>
          </div>
          {#if ctSaid}<p class="note mono">{ctSaid}</p>{/if}
          <h3>RSS feeds</h3>
          <p class="note">New items whose title has all the words are added as torrents. A feed's first check only marks what's already there.</p>
          {#each draft.feeds as f, i (i)}
            <div class="feed">
              <div class="grid2">
                <input bind:value={f.name} placeholder="Name" aria-label="Feed name" />
                <input bind:value={f.url} placeholder="https://…/rss" spellcheck="false" aria-label="Feed address" />
                <input bind:value={f.filter} placeholder="Must have (words, * ok)" spellcheck="false" aria-label="Words the title must have" />
                <input bind:value={f.exclude} placeholder="Must not have" spellcheck="false" aria-label="Words the title must not have" />
              </div>
              <div class="pick">
                <input bind:value={f.folder} placeholder="Folder (the download folder if empty)" spellcheck="false" aria-label="Folder" />
                <label class="check"><input type="checkbox" bind:checked={f.enabled} /> On</label>
                <button class="ghost sm" onclick={() => previewFeedItems(f.url, f.filter, f.exclude)}>Preview</button>
                <button class="ghost sm danger" onclick={() => draft && (draft.feeds = draft.feeds.filter((_, k) => k !== i))}>Remove</button>
              </div>
            </div>
          {/each}
          {#if feedPreview}
            <div class="feed-preview">
              <b>{feedPreview.length} items{feedPreview.length ? `, ${feedPreview.filter((x) => x.match).length} match` : ""}</b>
              {#each feedPreview.slice(0, 12) as it (it.guid)}<div class:muted={!it.match}>{it.match ? "✓" : "·"} {it.title}</div>{/each}
            </div>
          {/if}
          <div class="pick">
            <button class="ghost" onclick={() => draft && (draft.feeds = [...draft.feeds, { url: "", name: "", filter: "", exclude: "", folder: "", enabled: true }])}><Icon name="plus" size={15} />Add a feed</button>
            <label for="rm" class="inline">Check every</label><input id="rm" class="narrow" type="number" min="5" bind:value={draft.rss_minutes} /><span class="muted">minutes</span>
          </div>
        {:else if setTab === "rules"}
          <p class="note">Rules apply to new links, top to bottom; the first that matches the site and the file name wins.</p>
          {#each draft.rules as r, i (i)}
            <div class="rule">
              <label class="check"><input type="checkbox" bind:checked={r.enabled} aria-label="Rule on" /></label>
              <input bind:value={r.host} placeholder="Site (e.g. 1fichier.com), any if empty" spellcheck="false" aria-label="Site" />
              <input bind:value={r.name} placeholder="Name, e.g. *.nfo" spellcheck="false" aria-label="File name pattern" />
              <select bind:value={r.action} aria-label="What to do">
                <option value="folder">Save in folder</option>
                <option value="paused">Add paused</option>
                <option value="skip">Don't add</option>
              </select>
              <input bind:value={r.folder} placeholder="Folder" disabled={r.action !== "folder"} spellcheck="false" aria-label="Folder" />
              <button class="ghost sm danger" onclick={() => draft && (draft.rules = draft.rules.filter((_, k) => k !== i))} aria-label="Remove rule"><Icon name="trash" size={13} /></button>
            </div>
          {/each}
          <button class="ghost" onclick={() => draft && (draft.rules = [...draft.rules, { enabled: true, host: "", name: "", action: "folder", folder: "" }])}><Icon name="plus" size={15} />Add a rule</button>
          <h3>Convert when finished</h3>
          <p class="note">Videos and songs from matching sites are converted with FFmpeg into a new file beside them.</p>
          {#each draft.auto_convert as ac, i (i)}
            <div class="rule conv">
              <label class="check"><input type="checkbox" bind:checked={ac.enabled} aria-label="On" /></label>
              <input bind:value={ac.host} placeholder="Site (any if empty)" spellcheck="false" aria-label="Site" />
              <select bind:value={ac.category} aria-label="Type"><option value="">Video and music</option><option value="video">Video</option><option value="music">Music</option></select>
              <select bind:value={ac.preset} aria-label="Convert to">
                <option value="mp3">MP3</option><option value="m4a">M4A</option><option value="flac">FLAC</option><option value="mp4">MP4</option><option value="mp4-720">MP4 720p</option><option value="gif">GIF</option>
              </select>
              <label class="check small"><input type="checkbox" bind:checked={ac.delete_original} /> Delete original</label>
              <button class="ghost sm danger" onclick={() => draft && (draft.auto_convert = draft.auto_convert.filter((_, k) => k !== i))} aria-label="Remove"><Icon name="trash" size={13} /></button>
            </div>
          {/each}
          <button class="ghost" onclick={() => draft && (draft.auto_convert = [...draft.auto_convert, { enabled: true, host: "", category: "", preset: "mp3", delete_original: false }])}><Icon name="plus" size={15} />Add a conversion</button>
        {:else if setTab === "accounts"}
          <p class="note">A debrid account (Real-Debrid, AllDebrid, Premiumize) downloads file-host links (1fichier, Rapidgator, ddownload, katfile…) at full speed with every connection. Ultimate Download Manager keeps the key in Windows Credential Manager. Without one, those hosts open in a browser window where you wait and solve their captcha, and Ultimate Download Manager takes the file from there.</p>
          {#each accounts as a (a.id)}
            <div class="acc"><Icon name="done" size={15} /><b>{a.service}</b><span>{a.user}</span><button class="ghost sm" onclick={async () => { try { await api.removeAccount(a.id); accounts = accounts.filter((x) => x.id !== a.id); } catch (e) { say(String(e), true); } }}>Remove</button></div>
          {/each}
          <div class="grid-acc">
            <select bind:value={accService} aria-label="Service"><option>Real-Debrid</option><option>AllDebrid</option><option>Premiumize</option></select>
            <input type="password" bind:value={accKey} placeholder="API key" aria-label="API key" autocomplete="off" />
            <button class="ghost" onclick={addAccount} disabled={accBusy || !accKey.trim()}>{accBusy ? "Checking…" : "Add"}</button>
          </div>
          <button class="linkish" onclick={() => run(api.openWeb(KEY_PAGES[accService]))}>Where do I find my {accService} API key?</button>
        {:else if setTab === "remote"}
          <label class="check"><input type="checkbox" bind:checked={draft.remote_on} /> Let my phone add and control downloads over the home Wi-Fi (paired phones only)</label>
          {#if remote && draft.remote_on}
            <p class="note">Status: {remote.status}. Save first, then pair.</p>
            {#each remote.urls as u (u)}<p class="note mono">{u}</p>{/each}
            <button class="ghost" onclick={async () => { pairCode = await api.remotePair(); remote = await api.remoteInfo(); }}><Icon name="link" size={15} />Pair a phone</button>
            {#if pairCode}<p class="code">Open the address above on your phone and enter <b>{pairCode}</b> (works once, for a few minutes). Accept the certificate warning the first time: Ultimate Download Manager made its own certificate.</p>{/if}
            {#each remote.devices as [id, name] (id)}
              <div class="acc"><Icon name="done" size={15} /><b>{name}</b><span></span><button class="ghost sm" onclick={async () => { await api.remoteForget(id); remote = await api.remoteInfo(); }}>Forget</button></div>
            {/each}
          {/if}
        {:else if setTab === "app"}
          <label for="th">Theme</label>
          <select id="th" bind:value={draft.theme}><option value="dock">Dock (graphite and amber)</option><option value="dark">Obsidian (black)</option><option value="light">Light</option><option value="system">Follow Windows (Obsidian or Light)</option></select>
          <label for="lay">Main window layout</label>
          <select id="lay" bind:value={draft.layout}><option value="split">List and detail (larger)</option><option value="table">Table (compact)</option></select>
          <label class="check"><input type="checkbox" bind:checked={draft.autostart} /> Start with Windows (in the notification area)</label>
          <label class="check"><input type="checkbox" bind:checked={draft.txt_menu} /> "Download links with Ultimate Download Manager" when I right-click a text file</label>
          <label class="check"><input type="checkbox" bind:checked={draft.close_to_tray} /> Keep running in the notification area when the window is closed</label>
          <h3>Updates</h3>
          <label for="urepo">Releases repository on GitHub (public, holds only releases)</label>
          <input id="urepo" bind:value={draft.update_repo} placeholder="yourname/ultimate-download-manager-releases" spellcheck="false" />
          <label class="check"><input type="checkbox" bind:checked={draft.check_updates} /> Look for an update when Ultimate Download Manager starts</label>
          <div class="pick"><button class="ghost" onclick={checkUpdate}><Icon name="retry" size={15} />Check now</button>{#if update}<button class="primary" onclick={installUpdate}>Install {update.version}</button>{/if}</div>
          {#if updateMsg}<p class="note">{updateMsg}</p>{/if}
          <h3>Video tools</h3>
          <p class="note">yt-dlp {toolVersions?.ytdlp ?? "not installed"} · FFmpeg {toolVersions?.ffmpeg ?? "not installed"} · gallery-dl installs itself the first time you download a gallery</p>
          <button class="ghost" onclick={installTools} disabled={installing}><Icon name="retry" size={15} />{installing ? "Installing…" : toolVersions?.ytdlp ? "Update yt-dlp and FFmpeg" : "Install yt-dlp and FFmpeg"}</button>
          <h3>Backup</h3>
          <div class="pick">
            <button class="ghost" onclick={doExport}><Icon name="down" size={15} />Save settings and list…</button>
            <button class="ghost" onclick={doImport}><Icon name="open" size={15} />Restore from a backup…</button>
          </div>
          <h3>Crash reports</h3>
          <p class="note">If the app ever closes unexpectedly, a report is saved on this computer. Nothing is sent anywhere</p>
          <button class="ghost" onclick={() => api.openCrashReports().catch((e) => say(String(e), true))}><Icon name="folder" size={15} />Open crash reports</button>
        {/if}
      </div>
    </div>
    <div class="row-btns">
      <button class="ghost" onclick={() => onclose()}>Cancel</button>
      <button class="primary" onclick={saveSettings}>Save</button>
    </div>
  </div>
{/if}
