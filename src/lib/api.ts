// Typed calls into Ultimate Download Manager's Rust core (src-tauri/src/lib.rs).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type DlState = "queued" | "downloading" | "paused" | "done" | "failed";

export type Row = {
  id: number;
  url: string;
  name: string | null;
  dir: string;
  file: string | null;
  size: number | null;
  done: number;
  state: DlState;
  error: string | null;
  category: string;
  referer: string | null;
  created_at: number;
  finished_at: number | null;
  kind: string;
  display: string;
  speed: number;
  eta: number | null;
  conns: number;
  tags?: string;
  queue?: string;
};

export type Tick = {
  id: number;
  done: number;
  size: number | null;
  speed: number;
  eta: number | null;
  conns: number;
};

export type LinkInfo = {
  supported: boolean;
  kind: string;
  host: string | null;
  name: string | null;
  size: number | null;
  category: string | null;
};

export type Settings = {
  download_dir: string;
  max_parallel: number;
  connections: number;
  boost_connections: boolean;
  global_limit_kib: number;
  notify: boolean;
  close_to_tray: boolean;
  seed_when_done: boolean;
  extract_archives: boolean;
  delete_archives_after_extract: boolean;
  scan_downloads: boolean;
  sort_by_type: boolean;
  watch_clipboard: boolean;
  adapter: string;
  schedule_enabled: boolean;
  schedule_start: string;
  schedule_stop: string;
  schedule_days: boolean[];
  when_done: string;
  slow_enabled: boolean;
  slow_from: string;
  slow_to: string;
  slow_limit_kib: number;
  remote_on: boolean;
  remote_port: number;
  update_repo: string;
  check_updates: boolean;
  torrent_port: number;
  torrent_dht: boolean;
  torrent_upnp: boolean;
  torrent_down_kib: number;
  torrent_up_kib: number;
  seed_ratio: number;
  seed_minutes: number;
  torrent_blocklist: string;
  feeds: Feed[];
  rss_minutes: number;
  proxy: string;
  reconnect_command: string;
  archive_passwords: string[];
  rules: Rule[];
  theme: "dark" | "light" | "system" | "dock";
  theme_dock_seen?: boolean;
  autostart: boolean;
  sound: boolean;
  ask_where: boolean;
  ask_skip_types: string[];
  last_dirs: Record<string, string>;
  layout: "split" | "table";
  handle_torrents: boolean;
  txt_menu: boolean;
  hotkey: string;
  complete_window: boolean;
  complete_min_mb: number;
  asked_torrents: boolean;
  site_limits: { host: string; kib: number }[];
  queues: Queue[];
  main_paused: boolean;
  auto_convert: { enabled: boolean; host: string; category: string; preset: string; delete_original: boolean }[];
  forget_after_days: number;
};

export type Grabbed = {
  url: string;
  kind: "file" | "video" | "torrent";
  name: string;
  size: number | null;
  status: "online" | "offline" | "unknown";
  note: string | null;
  duplicate: boolean;
  package: string;
};

export function onClipboard(f: (links: string[]) => void): Promise<UnlistenFn> {
  return listen<string[]>("udm://clipboard", (e) => f(e.payload));
}

export type TorrentMeta = {
  hash: string;
  name: string;
  total: number;
  files: { index: number; path: string; size: number }[];
};

export type Sub = { id: number; url: string; title: string; quality: string; every_min: number; last_check: number | null; last_error: string | null; enabled: boolean };

export type RemoteInfo = { status: string; urls: string[]; fingerprint: string | null; devices: [string, string][]; code: string | null };

export function onUpdate(f: (u: { version: string; notes: string | null }) => void): Promise<UnlistenFn> {
  return listen<{ version: string; notes: string | null }>("udm://update", (e) => f(e.payload));
}

export type Account = { id: number; service: string; user: string; key_page: string };

export type ToolsStatus ={ ytdlp: string | null; ffmpeg: string | null };

export type Choice = { label: string; selector: string; ext: string; size: number | null; audio: boolean; holds_subs: boolean };
export type SubTrack = { code: string; name: string; automatic: boolean };
export type Entry = { index: number; url: string; title: string; duration: number | null; unavailable: boolean };

export type Lookup =
  | {
      kind: "video"; title: string; duration: number | null; choices: Choice[]; subs: SubTrack[]; needs_ffmpeg: boolean;
      thumbnail?: string | null; audio_langs?: string[]; chapters?: boolean;
    }
  | { kind: "list"; title: string; total: number | null; entries: Entry[]; capped: boolean };

export type VideoOpts = {
  selector: string;
  ext: string;
  expected: number | null;
  subs: string[];
  automatic_subs: boolean;
  embed_subs: boolean;
  section?: string;
  sponsorblock?: boolean;
  chapters?: boolean;
};

export type Queue = { id: string; name: string; parallel: number; paused: boolean; from: string; to: string };

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export type Feed = { url: string; name: string; filter: string; exclude: string; folder: string; enabled: boolean };
export type Rule = { enabled: boolean; host: string; name: string; action: "folder" | "skip" | "paused"; folder: string };
export type FeedItem = { title: string; link: string; guid: string };
export type ItemExtras = {
  limit_kib: number; priority: number; checksum: string | null; password: boolean;
  seed_ratio?: number | null; seed_minutes?: number | null; mirrors?: string[]; rule?: string | null;
};
export type Stats = {
  files: number;
  bytes: number;
  months: { month: string; files: number; bytes: number }[];
  types: { type: string; files: number; bytes: number }[];
};

export type ContentFile = { index: number; path: string; size: number; done: number | null; wanted: boolean };
export type PeerView = { addr: string; client: string | null; fetched: number; uploaded: number; kind: string | null };
export type TorrentDetails = {
  hash: string;
  live: boolean;
  files: ContentFile[];
  peers: PeerView[];
  peers_seen: number;
  trackers: string[];
  pieces: number[];
  piece_count: number;
  piece_size: number;
  uploaded: number;
  up_speed: number;
  phase: string;
};
export type ServerView = {
  ip: string;
  country: string | null;
  flag: [number, number] | null;
  rtt_ms: number | null;
  proxied: boolean;
  cdn: string | null;
};
export type FileDetails = {
  segments: [number, number, number][];
  connections: [number, number, boolean][];
  resumable: boolean | null;
  server?: ServerView | null;
};
export type Details =
  | { kind: "torrent"; torrent: TorrentDetails; sequential: boolean; item: ItemExtras }
  | { kind: "file"; file: FileDetails | null; item: ItemExtras };

export const api = {
  clearList: (which: "finished" | "failed" | "missing" | "all", deleteFiles: boolean) => invoke<number>("clear_list", { which, deleteFiles }),
  redownload: (ids: number[]) => invoke<void>("redownload", { ids }),
  openWith: (id: number) => invoke<void>("open_with", { id }),
  setTags: (ids: number[], tags: string) => invoke<void>("set_tags", { ids, tags }),
  setQueue: (ids: number[], queue: string) => invoke<void>("set_queue", { ids, queue }),
  addMirrors: (id: number, urls: string[]) => invoke<void>("add_mirrors", { id, urls }),
  torrentSeeding: (id: number, ratio: number | null, minutes: number | null) => invoke<void>("torrent_seeding", { id, ratio, minutes }),
  createTorrent: (source: string, trackers: string, out: string, seed: boolean) => invoke<string>("create_torrent", { source, trackers, out, seed }),
  portCheck: () => invoke<{ port: number; reachable: boolean }>("torrent_port_check"),
  showProgress: (id: number) => invoke<void>("show_progress", { id }),
  assist: (id: number) => invoke<void>("assist_download", { id }),
  addGallery: (url: string, dir: string | null, limit: number) => invoke<number>("add_gallery", { url, dir, limit }),
  setItemLimit: (ids: number[], kib: number) => invoke<void>("set_item_limit", { ids, kib }),
  moveInQueue: (ids: number[], top: boolean) => invoke<void>("move_in_queue", { ids, top }),
  nudge: (id: number, up: boolean) => invoke<void>("nudge_download", { id, up }),
  setChecksum: (id: number, text: string) => invoke<boolean | null>("set_checksum", { id, text }),
  setArchivePassword: (id: number, password: string) => invoke<void>("set_archive_password", { id, password }),
  refreshAddress: (id: number, url: string) => invoke<void>("refresh_address", { id, url }),
  moveFile: (id: number, to: string) => invoke<string>("move_file", { id, to }),
  convertPresets: () => invoke<[string, string][]>("convert_presets"),
  convertFile: (id: number, preset: string) => invoke<string>("convert_file", { id, preset }),
  torrentAction: (id: number, action: "recheck" | "reannounce" | "sequential-on" | "sequential-off") =>
    invoke<void>("torrent_action", { id, action }),
  torrentAddTrackers: (id: number, text: string) => invoke<number>("torrent_add_trackers", { id, text }),
  stats: () => invoke<Stats>("download_stats"),
  exportBackup: (path: string) => invoke<number>("export_backup", { path }),
  importBackup: (path: string) => invoke<number>("import_backup", { path }),
  checkFeed: (url: string) => invoke<string[]>("check_feed", { url }),
  previewFeed: (url: string) => invoke<FeedItem[]>("preview_feed", { url }),
  details: (id: number) => invoke<Details>("download_details", { id }),
  torrentChoose: (id: number, files: number[]) => invoke<void>("torrent_choose", { id, files }),
  status: () => invoke<{ version: string; built: number; data_dir: string | null }>("core_status"),
  probe: (url: string) => invoke<LinkInfo>("probe_link", { url }),
  list: () => invoke<Row[]>("list_downloads"),
  add: (url: string, dir: string | null, name: string | null, start = true) =>
    invoke<number>("add_download", { url, dir, name, start }),
  pause: (ids: number[]) => invoke<void>("pause_downloads", { ids }),
  resume: (ids: number[]) => invoke<void>("resume_downloads", { ids }),
  remove: (ids: number[], deleteFiles: boolean) =>
    invoke<void>("remove_downloads", { ids, deleteFiles }),
  open: (id: number) => invoke<void>("open_download", { id }),
  reveal: (id: number) => invoke<void>("reveal_download", { id }),
  siteCheck: (url: string) => invoke<void>("open_site_check", { url }),
  browserDownload: (id: number) => invoke<void>("browser_download", { id }),
  extensionInfo: () =>
    invoke<{ chromium: string | null; firefox: string | null; host: { Ok?: string; Err?: string } }>("extension_info"),
  openFolder: (path: string) => invoke<void>("open_folder", { path }),
  openCrashReports: () => invoke<void>("open_crash_reports"),
  grab: (text: string) => invoke<Grabbed[]>("grab_links", { text }),
  addGrabbed: (picks: { url: string; kind: string; name: string }[], dir: string | null, start: boolean) =>
    invoke<number>("add_grabbed", { picks, dir, start }),
  torrentMeta: (source: string) => invoke<TorrentMeta>("torrent_meta", { source }),
  addTorrent: (hash: string, files: number[] | null, dir: string | null) =>
    invoke<number>("add_torrent", { hash, files, dir }),
  adapters: () => invoke<{ name: string; description: string | null; up: boolean; addresses: string[] }[]>("list_adapters"),
  subs: () => invoke<Sub[]>("list_subs"),
  addSub: (url: string, quality: string, everyMin: number, latest: number) =>
    invoke<Sub>("add_sub", { url, quality, everyMin, latest }),
  removeSub: (id: number) => invoke<void>("remove_sub", { id }),
  checkSub: (id: number) => invoke<number>("check_sub", { id }),
  preview: (id: number) => invoke<void>("preview", { id }),
  remoteInfo: () => invoke<RemoteInfo>("remote_info"),
  remotePair: () => invoke<string>("remote_pair"),
  remoteForget: (id: string) => invoke<void>("remote_forget", { id }),
  checkUpdate: () => invoke<{ version: string; notes: string | null } | null>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
  accounts: () => invoke<Account[]>("list_accounts"),
  addAccount: (service: string, key: string) => invoke<Account>("add_account", { service, key }),
  removeAccount: (id: number) => invoke<void>("remove_account", { id }),
  openWeb: (url: string) => invoke<void>("open_web", { url }),
  toolsStatus: () => invoke<ToolsStatus>("tools_status"),
  installTools: () => invoke<ToolsStatus>("install_tools"),
  videoLookup: (url: string) => invoke<Lookup>("video_lookup", { url }),
  addVideo: (page: string, title: string, opts: VideoOpts, dir: string | null) =>
    invoke<number>("add_video", { page, title, opts, dir }),
  addPlaylist: (entries: { url: string; title: string }[], quality: string, dir: string | null) =>
    invoke<number>("add_playlist", { entries, quality, dir }),
  settings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("set_settings", { settings }),
};

export function onChanged(f: () => void): Promise<UnlistenFn> {
  return listen("udm://changed", () => f());
}

/** The text a failed download's error starts with when the site wants a real browser first. */
export const CHECK_NEEDED = "Site check needed: ";
export const ASSIST_NEEDED = "Needs you: ";

export function onToast(f: (t: { text: string; bad: boolean }) => void): Promise<UnlistenFn> {
  return listen<{ text: string; bad: boolean }>("udm://toast", (e) => f(e.payload));
}

export function onShow(f: (what: string) => void): Promise<UnlistenFn> {
  return listen<string>("udm://show", (e) => f(e.payload));
}

export function onSession(f: (host: string) => void): Promise<UnlistenFn> {
  return listen<{ host: string }>("udm://session", (e) => f(e.payload.host));
}

export function onProgress(f: (ticks: Tick[]) => void): Promise<UnlistenFn> {
  return listen<Tick[]>("udm://progress", (e) => f(e.payload));
}
