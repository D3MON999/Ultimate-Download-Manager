<script lang="ts">
  import Icon from "$lib/Icon.svelte";
  import { api, type TorrentMeta } from "$lib/api";
  import * as fmt from "$lib/format";
  import { open as pickFile } from "@tauri-apps/plugin-dialog";
  import { onMount, tick } from "svelte";

  let {
    source: initialSource = "",
    dir: initialDir = "",
    onclose,
    onsay,
    browse,
  }: {
    source?: string;
    dir?: string;
    onclose: () => void;
    onsay: (text: string, bad?: boolean) => void;
    browse: (current: string) => Promise<string | null>;
  } = $props();

  // svelte-ignore state_referenced_locally
  let source = $state(initialSource);
  // svelte-ignore state_referenced_locally
  let dir = $state(initialDir);
  let busy = $state(false);
  let error = $state("");
  let meta = $state<TorrentMeta | null>(null);
  let picked = $state<Set<number>>(new Set());
  let input = $state<HTMLInputElement | null>(null);

  const chosenSize = $derived(meta ? meta.files.filter((f) => picked.has(f.index)).reduce((a, f) => a + f.size, 0) : 0);

  async function lookUp() {
    if (!source.trim()) return;
    busy = true;
    error = "";
    meta = null;
    try {
      meta = await api.torrentMeta(source.trim());
      picked = new Set(meta.files.map((f) => f.index));
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function openFile() {
    const f = await pickFile({ filters: [{ name: "Torrent", extensions: ["torrent"] }], title: "Open a .torrent file" });
    if (typeof f === "string") {
      source = f;
      lookUp();
    }
  }

  async function start() {
    if (!meta || picked.size === 0) return;
    busy = true;
    try {
      const all = picked.size === meta.files.length;
      await api.addTorrent(meta.hash, all ? null : [...picked], dir || null);
      onsay(`Torrent started: ${meta.name}`);
      onclose();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  onMount(async () => {
    await tick();
    input?.focus();
    if (initialSource) lookUp();
  });
</script>

<div class="scrim" onclick={onclose} role="presentation"></div>
<div class="dialog" role="dialog" aria-modal="true" aria-labelledby="tor-title">
  <div class="dialog-head">
    <h2 id="tor-title">Torrent</h2>
    <button class="x" onclick={onclose} aria-label="Close"><Icon name="close" size={16} /></button>
  </div>
  <label for="tsrc">Magnet link</label>
  <div class="pick">
    <input id="tsrc" bind:this={input} bind:value={source} placeholder="magnet:?xt=urn:btih:…" spellcheck="false" autocomplete="off" onkeydown={(e) => e.key === "Enter" && lookUp()} />
    <button class="ghost" onclick={lookUp} disabled={busy || !source.trim()}><Icon name="search" size={15} />Look up</button>
    <button class="ghost" onclick={openFile}><Icon name="file" size={15} />.torrent file</button>
  </div>
  {#if busy && !meta}
    <p class="note" role="status">Asking the swarm for the file list… (up to a minute for a magnet link)</p>
  {:else if error}
    <p class="note bad" role="alert">{error}</p>
  {/if}

  {#if meta}
    <div class="head"><b>{meta.name}</b><span class="mono">{fmt.bytes(chosenSize)} of {fmt.bytes(meta.total)}</span></div>
    <div class="tools">
      <button class="ghost sm" onclick={() => meta && (picked = new Set(meta.files.map((f) => f.index)))}>All</button>
      <button class="ghost sm" onclick={() => (picked = new Set())}>None</button>
      <span class="note">{picked.size} of {meta.files.length} files</span>
    </div>
    <ul class="files">
      {#each meta.files as f (f.index)}
        <li>
          <label>
            <input type="checkbox" checked={picked.has(f.index)}
              onchange={(ev) => { const s = new Set(picked); (ev.currentTarget as HTMLInputElement).checked ? s.add(f.index) : s.delete(f.index); picked = s; }} />
            <span class="fp" title={f.path}>{f.path}</span>
            <span class="mono">{fmt.bytes(f.size)}</span>
          </label>
        </li>
      {/each}
    </ul>
    <label for="tdir">Save in</label>
    <div class="pick">
      <input id="tdir" bind:value={dir} spellcheck="false" autocomplete="off" />
      <button class="ghost" onclick={async () => { const p = await browse(dir); if (p) dir = p; }}><Icon name="folder" size={15} />Browse</button>
    </div>
    <p class="note">Only download what you have the right to. A finished torrent stops sharing unless you change that in Settings.</p>
    <div class="row-btns">
      <button class="ghost" onclick={onclose}>Cancel</button>
      <button class="primary" onclick={start} disabled={busy || picked.size === 0}>Download</button>
    </div>
  {/if}
</div>

<style>
  .scrim { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.72); backdrop-filter: blur(4px); }
  .dialog { position: fixed; left: 50%; top: 8vh; transform: translateX(-50%); width: min(680px, calc(100vw - 32px)); max-height: 84vh; overflow: auto; background: var(--card); border: 1px solid var(--line2); border-radius: var(--r-tile); box-shadow: 0 30px 80px rgba(0, 0, 0, 0.8); padding: var(--s5); display: flex; flex-direction: column; gap: var(--s2); }
  .dialog-head { display: flex; justify-content: space-between; align-items: center; }
  h2 { margin: 0; font: 600 20px var(--head); }
  .x { border: 0; background: transparent; color: var(--muted); width: 32px; height: 32px; border-radius: 8px; display: grid; place-items: center; }
  .x:hover { color: var(--text); background: var(--card-hover); }
  label { color: var(--muted); font-size: 12.5px; margin-top: 4px; }
  input:not([type="checkbox"]) { width: 100%; height: 40px; padding: 0 12px; background: var(--field); border: 1px solid var(--line2); border-radius: 10px; outline: none; color: var(--text); font: inherit; }
  input:focus { border-color: var(--text); }
  input[type="checkbox"] { width: 16px; height: 16px; accent-color: var(--text); margin: 0; flex: none; }
  .pick { display: flex; gap: var(--s2); }
  .ghost, .primary { height: 36px; padding: 0 14px; border-radius: 10px; font-weight: 500; display: inline-flex; align-items: center; gap: 6px; flex: none; }
  .ghost { background: transparent; border: 1px solid var(--line2); }
  .ghost.sm { height: 30px; padding: 0 12px; }
  .primary { background: var(--hero); color: var(--hero-text); border: 1px solid var(--hero); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .note { margin: 0; color: var(--muted); font-size: 13px; }
  .note.bad { color: var(--down); }
  .head { display: flex; justify-content: space-between; gap: var(--s3); margin-top: var(--s2); }
  .head b { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .mono { font-family: var(--mono); font-size: 12px; color: var(--muted); white-space: nowrap; }
  .tools { display: flex; gap: var(--s2); align-items: center; }
  .files { list-style: none; margin: 0; padding: 0; max-height: 36vh; overflow: auto; border: 1px solid var(--line); border-radius: 10px; }
  .files label { display: flex; align-items: center; gap: 10px; padding: 8px 12px; margin: 0; border-bottom: 1px solid var(--line); color: var(--text-2); font-size: 13.5px; cursor: pointer; }
  .fp { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row-btns { display: flex; justify-content: flex-end; gap: var(--s2); margin-top: var(--s3); }
</style>
