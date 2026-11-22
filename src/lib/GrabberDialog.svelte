<script lang="ts">
  import Icon from "$lib/Icon.svelte";
  import { api, type Grabbed } from "$lib/api";
  import * as fmt from "$lib/format";
  import { onMount, tick } from "svelte";

  let {
    text: initialText = "",
    dir: initialDir = "",
    onclose,
    onsay,
    browse,
    ontorrent,
  }: {
    text?: string;
    dir?: string;
    onclose: () => void;
    onsay: (text: string, bad?: boolean) => void;
    browse: (current: string) => Promise<string | null>;
    ontorrent: (magnet: string) => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let text = $state(initialText);
  // svelte-ignore state_referenced_locally
  let dir = $state(initialDir);
  let busy = $state(false);
  let error = $state("");
  let items = $state<Grabbed[] | null>(null);
  let picked = $state<Set<number>>(new Set());
  let area = $state<HTMLTextAreaElement | null>(null);

  type Pkg = { name: string; rows: { g: Grabbed; i: number }[]; size: number };
  const packages = $derived.by(() => {
    const map = new Map<string, Pkg>();
    (items ?? []).forEach((g, i) => {
      const p = map.get(g.package) ?? { name: g.package, rows: [], size: 0 };
      p.rows.push({ g, i });
      p.size += g.size ?? 0;
      map.set(g.package, p);
    });
    return [...map.values()];
  });
  const chosen = $derived((items ?? []).filter((_, i) => picked.has(i)));
  const torrents = $derived(chosen.filter((g) => g.kind === "torrent"));

  async function check() {
    if (!text.trim()) return;
    busy = true;
    error = "";
    items = null;
    try {
      const got = await api.grab(text);
      items = got;
      picked = new Set(got.map((g, i) => [g, i] as const).filter(([g]) => g.status !== "offline" && !g.duplicate).map(([, i]) => i));
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function toggle(i: number, on: boolean) {
    const s = new Set(picked);
    on ? s.add(i) : s.delete(i);
    picked = s;
  }

  function togglePkg(p: Pkg, on: boolean) {
    const s = new Set(picked);
    p.rows.forEach(({ i }) => (on ? s.add(i) : s.delete(i)));
    picked = s;
  }

  async function add(start: boolean) {
    busy = true;
    try {
      const picks = chosen.filter((g) => g.kind !== "torrent").map((g) => ({ url: g.url, kind: g.kind, name: g.kind === "video" ? "" : g.name }));
      const n = picks.length ? await api.addGrabbed(picks, dir || null, start) : 0;
      onsay(`${n} added${torrents.length ? `; ${torrents.length} torrent${torrents.length === 1 ? "" : "s"} to set up` : ""}`);
      onclose();
      if (torrents.length) ontorrent(torrents[0].url);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  const statusIcon = (g: Grabbed) => (g.status === "online" ? "done" : g.status === "offline" ? "close" : g.kind === "video" ? "video" : g.kind === "torrent" ? "magnet" : "file");

  onMount(async () => {
    await tick();
    area?.focus();
    if (initialText) check();
  });
</script>

<div class="scrim" onclick={onclose} role="presentation"></div>
<div class="dialog" role="dialog" aria-modal="true" aria-labelledby="grab-title">
  <div class="dialog-head">
    <h2 id="grab-title">Link grabber</h2>
    <button class="x" onclick={onclose} aria-label="Close"><Icon name="close" size={16} /></button>
  </div>
  {#if !items}
    <label for="grab">Paste links, a web page's text, or numbered patterns like <code>https://site/file[1-20].zip</code></label>
    <textarea id="grab" bind:this={area} bind:value={text} rows="9" spellcheck="false" placeholder="https://…"></textarea>
    {#if busy}<p class="note" role="status">Checking the links…</p>{:else if error}<p class="note bad" role="alert">{error}</p>{/if}
    <div class="row-btns">
      <button class="ghost" onclick={onclose}>Cancel</button>
      <button class="primary" onclick={check} disabled={busy || !text.trim()}>Find and check links</button>
    </div>
  {:else}
    <div class="sum">
      <span>{items.length} link{items.length === 1 ? "" : "s"} in {packages.length} package{packages.length === 1 ? "" : "s"}</span>
      <span class="mono">{picked.size} chosen · {fmt.bytes(chosen.reduce((a, g) => a + (g.size ?? 0), 0))}</span>
    </div>
    <div class="pkgs">
      {#each packages as p (p.name)}
        {@const all = p.rows.every(({ i }) => picked.has(i))}
        <div class="pkg">
          <label class="ph">
            <input type="checkbox" checked={all} onchange={(e) => togglePkg(p, (e.currentTarget as HTMLInputElement).checked)} />
            <Icon name="archive" size={15} />
            <b>{p.name}</b>
            <span class="mono">{p.rows.length} · {p.size ? fmt.bytes(p.size) : "—"}</span>
          </label>
          {#if p.rows.length > 1 && p.rows.some(({ g }) => /\.(part\d+\.rar|r\d\d|7z\.\d{3}|zip\.\d{3}|z\d\d)$/i.test(g.name))}
            <p class="pnote">Saved together in a folder named {p.name}, unpacked when every part has arrived</p>
          {/if}
          {#each p.rows as { g, i } (g.url)}
            <label class="gr" class:off={g.status === "offline"}>
              <input type="checkbox" checked={picked.has(i)} onchange={(e) => toggle(i, (e.currentTarget as HTMLInputElement).checked)} />
              <span class="st {g.status}"><Icon name={statusIcon(g)} size={14} /></span>
              <span class="gn" title={g.url}>{g.name}</span>
              {#if g.duplicate}<span class="tag">in list</span>{/if}
              {#if g.note}<span class="tag bad" title={g.note}>{g.status === "offline" ? "offline" : "?"}</span>{/if}
              <span class="mono">{g.size ? fmt.bytes(g.size) : g.kind === "file" ? "" : g.kind}</span>
            </label>
          {/each}
        </div>
      {/each}
    </div>
    {#if error}<p class="note bad" role="alert">{error}</p>{/if}
    <label for="gdir">Save in</label>
    <div class="pick">
      <input id="gdir" bind:value={dir} spellcheck="false" autocomplete="off" />
      <button class="ghost" onclick={async () => { const p = await browse(dir); if (p) dir = p; }}><Icon name="folder" size={15} />Browse</button>
    </div>
    <div class="row-btns">
      <button class="ghost left" onclick={() => (items = null)}>Back</button>
      <button class="ghost" onclick={() => add(false)} disabled={busy || picked.size === 0}>Add paused</button>
      <button class="primary" onclick={() => add(true)} disabled={busy || picked.size === 0}>Download {picked.size}</button>
    </div>
  {/if}
</div>

<style>
  .scrim { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.72); backdrop-filter: blur(4px); }
  .dialog { position: fixed; left: 50%; top: 7vh; transform: translateX(-50%); width: min(760px, calc(100vw - 32px)); max-height: 86vh; overflow: auto; background: var(--card); border: 1px solid var(--line2); border-radius: var(--r-tile); box-shadow: 0 30px 80px rgba(0, 0, 0, 0.8); padding: var(--s5); display: flex; flex-direction: column; gap: var(--s2); }
  .dialog-head { display: flex; justify-content: space-between; align-items: center; }
  h2 { margin: 0; font: 600 20px var(--head); }
  .x { border: 0; background: transparent; color: var(--muted); width: 32px; height: 32px; border-radius: 8px; display: grid; place-items: center; }
  label { color: var(--muted); font-size: 12.5px; }
  code { font: 12px var(--mono); background: var(--field); border: 1px solid var(--line2); border-radius: 6px; padding: 1px 6px; }
  textarea { width: 100%; padding: 12px; background: var(--field); border: 1px solid var(--line2); border-radius: 10px; color: var(--text); font: 13px var(--mono); resize: vertical; outline: none; }
  textarea:focus, input:focus { border-color: var(--text); }
  input:not([type="checkbox"]) { width: 100%; height: 40px; padding: 0 12px; background: var(--field); border: 1px solid var(--line2); border-radius: 10px; outline: none; color: var(--text); font: inherit; }
  input[type="checkbox"] { width: 16px; height: 16px; accent-color: var(--text); margin: 0; flex: none; }
  .pick { display: flex; gap: var(--s2); }
  .ghost, .primary { height: 36px; padding: 0 14px; border-radius: 10px; font-weight: 500; display: inline-flex; align-items: center; gap: 6px; flex: none; }
  .ghost { background: transparent; border: 1px solid var(--line2); }
  .primary { background: var(--hero); color: var(--hero-text); border: 1px solid var(--hero); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .note { margin: 0; color: var(--muted); font-size: 13px; }
  .note.bad { color: var(--down); }
  .row-btns { display: flex; justify-content: flex-end; gap: var(--s2); margin-top: var(--s3); }
  .row-btns .left { margin-right: auto; }
  .sum { display: flex; justify-content: space-between; color: var(--muted); font-size: 13px; }
  .mono { font-family: var(--mono); font-size: 12px; color: var(--muted); white-space: nowrap; }
  .pkgs { border: 1px solid var(--line); border-radius: 10px; max-height: 44vh; overflow: auto; }
  .pkg + .pkg { border-top: 1px solid var(--line2); }
  .ph { display: flex; align-items: center; gap: 10px; padding: 9px 12px; background: #080808; color: var(--text); cursor: pointer; }
  .ph b { flex: 1; font-weight: 600; font-size: 13.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .gr { display: flex; align-items: center; gap: 10px; padding: 7px 12px 7px 26px; border-top: 1px solid var(--line); color: var(--text-2); font-size: 13px; cursor: pointer; }
  .gr.off { opacity: 0.55; }
  .gn { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .st { display: grid; place-items: center; color: var(--muted); }
  .st.online { color: var(--up); }
  .st.offline { color: var(--down); }
  .tag { font-size: 11px; padding: 1px 7px; border-radius: 999px; border: 1px solid var(--line3); color: var(--muted); }
  .tag.bad { border-color: rgba(246, 70, 93, 0.5); color: var(--down); }
  .pnote { margin: 0 0 4px 30px; font-size: 12px; color: var(--muted); }
</style>
