<script lang="ts">
  import Icon from "$lib/Icon.svelte";
  import { api, type Choice, type Lookup } from "$lib/api";
  import * as fmt from "$lib/format";
  import { onMount, tick } from "svelte";

  let {
    url: initialUrl = "",
    dir: initialDir = "",
    onclose,
    onsay,
    browse,
  }: {
    url?: string;
    dir?: string;
    onclose: () => void;
    onsay: (text: string, bad?: boolean) => void;
    browse: (current: string) => Promise<string | null>;
  } = $props();

  // Seeded once when the dialog opens (it's created afresh each time).
  // svelte-ignore state_referenced_locally
  let url = $state(initialUrl);
  // svelte-ignore state_referenced_locally
  let dir = $state(initialDir);
  let busy = $state(false);
  let error = $state("");
  let result = $state<Lookup | null>(null);
  let pick = $state(0);
  let sub = $state("");
  let embed = $state(true);
  let picked = $state<Set<number>>(new Set());
  let quality = $state("best");
  let input = $state<HTMLInputElement | null>(null);

  const choice = $derived(result?.kind === "video" ? result.choices[pick] : null);

  async function lookUp() {
    if (!url.trim()) return;
    busy = true;
    error = "";
    result = null;
    try {
      const r = await api.videoLookup(url.trim());
      result = r;
      pick = 0;
      sub = "";
      if (r.kind === "list") picked = new Set(r.entries.filter((e) => !e.unavailable).map((e) => e.index));
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function download() {
    if (!result) return;
    busy = true;
    try {
      if (result.kind === "video" && choice) {
        const s = result.subs.find((t) => t.code === sub);
        await api.addVideo(url.trim(), result.title, {
          selector: choice.selector,
          ext: choice.ext,
          expected: choice.size,
          subs: s && !choice.audio ? [s.code] : [],
          automatic_subs: s?.automatic ?? false,
          embed_subs: embed && choice.holds_subs,
        }, dir || null);
        onsay(`Added: ${result.title}`);
      } else if (result.kind === "list") {
        const entries = result.entries.filter((e) => picked.has(e.index)).map((e) => ({ url: e.url, title: e.title }));
        const n = await api.addPlaylist(entries, quality, dir || null);
        onsay(`${n} video${n === 1 ? "" : "s"} added from ${result.title}`);
      }
      onclose();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function label(c: Choice): string {
    return c.label.replace("Audio only original", "Audio only (original)");
  }

  onMount(async () => {
    await tick();
    input?.focus();
    if (initialUrl) lookUp();
  });
</script>

<div class="scrim" onclick={onclose} role="presentation"></div>
<div class="dialog wide" role="dialog" aria-modal="true" aria-labelledby="vid-title">
  <div class="dialog-head">
    <h2 id="vid-title">Video &amp; audio</h2>
    <button class="x" onclick={onclose} aria-label="Close"><Icon name="close" size={16} /></button>
  </div>
  <label for="vurl">Page address</label>
  <div class="pick">
    <input id="vurl" bind:this={input} bind:value={url} placeholder="YouTube, Instagram, X, TikTok, Reddit, Twitch… or a playlist" spellcheck="false" autocomplete="off" onkeydown={(e) => e.key === "Enter" && lookUp()} />
    <button class="ghost" onclick={lookUp} disabled={busy || !url.trim()}><Icon name="search" size={15} />Look up</button>
  </div>

  {#if busy && !result}
    <p class="note" role="status">Looking up the page with yt-dlp… The first time, Ultimate Download Manager installs yt-dlp and FFmpeg (about 100 MB), so this takes a minute.</p>
  {:else if error}
    <p class="note bad" role="alert">{error}</p>
  {/if}

  {#if result?.kind === "video"}
    <div class="vtitle"><b>{result.title}</b>{#if result.duration}<span class="mono">{fmt.eta(Math.round(result.duration))}</span>{/if}</div>
    {#if result.needs_ffmpeg}<p class="note">Install FFmpeg (Settings) for higher qualities: the site keeps picture and sound apart.</p>{/if}
    <div class="choices" role="radiogroup" aria-label="Quality">
      {#each result.choices as c, i (c.selector + i)}
        <label class="choice" class:on={pick === i}>
          <input type="radio" name="q" value={i} bind:group={pick} />
          <Icon name={c.audio ? "music" : "video"} size={15} />
          <span class="cl">{label(c)}</span>
          <span class="mono">{c.size ? `~${fmt.bytes(c.size)}` : ""}</span>
        </label>
      {/each}
    </div>
    {#if result.subs.length && choice && !choice.audio}
      <div class="grid2">
        <div>
          <label for="subs">Subtitles</label>
          <select id="subs" bind:value={sub}>
            <option value="">None</option>
            {#each result.subs as s (s.code + s.automatic)}
              <option value={s.code}>{s.name}{s.automatic ? " (automatic)" : ""}</option>
            {/each}
          </select>
        </div>
        {#if sub && choice.holds_subs}
          <label class="check end"><input type="checkbox" bind:checked={embed} /> Put them inside the video</label>
        {/if}
      </div>
    {/if}
  {:else if result?.kind === "list"}
    <div class="vtitle"><b>{result.title}</b><span>{result.entries.length}{result.total && result.total > result.entries.length ? ` of ${result.total}` : ""} videos</span></div>
    <div class="list-tools">
      <button class="ghost sm" onclick={() => result?.kind === "list" && (picked = new Set(result.entries.filter((e) => !e.unavailable).map((e) => e.index)))}>All</button>
      <button class="ghost sm" onclick={() => (picked = new Set())}>None</button>
      <select bind:value={quality} aria-label="Quality for all">
        <option value="best">Best quality</option>
        <option value="1080">Up to 1080p</option>
        <option value="720">Up to 720p</option>
        <option value="480">Up to 480p</option>
        <option value="mp3">Audio only (MP3)</option>
      </select>
    </div>
    <ul class="entries">
      {#each result.entries as e (e.index)}
        <li class:gone={e.unavailable}>
          <label>
            <input type="checkbox" disabled={e.unavailable} checked={picked.has(e.index)}
              onchange={(ev) => { const s = new Set(picked); (ev.currentTarget as HTMLInputElement).checked ? s.add(e.index) : s.delete(e.index); picked = s; }} />
            <span class="idx mono">{e.index}</span>
            <span class="et">{e.title}</span>
            <span class="mono">{e.duration ? fmt.eta(Math.round(e.duration)) : ""}</span>
          </label>
        </li>
      {/each}
    </ul>
    {#if result.capped}<p class="note">The list was long: Ultimate Download Manager shows the first part.</p>{/if}
  {/if}

  {#if result}
    <label for="vdir">Save in</label>
    <div class="pick">
      <input id="vdir" bind:value={dir} spellcheck="false" autocomplete="off" />
      <button class="ghost" onclick={async () => { const p = await browse(dir); if (p) dir = p; }}><Icon name="folder" size={15} />Browse</button>
    </div>
    <div class="row-btns">
      <button class="ghost" onclick={onclose}>Cancel</button>
      <button class="primary" onclick={download} disabled={busy || (result.kind === "list" && picked.size === 0)}>
        {result.kind === "list" ? `Download ${picked.size}` : "Download"}
      </button>
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
  input:not([type="checkbox"]):not([type="radio"]), select { width: 100%; height: 40px; padding: 0 12px; background: var(--field); border: 1px solid var(--line2); border-radius: 10px; outline: none; color: var(--text); font: inherit; }
  input:focus, select:focus { border-color: var(--text); }
  input[type="checkbox"], input[type="radio"] { width: 16px; height: 16px; accent-color: var(--text); margin: 0; }
  .pick { display: flex; gap: var(--s2); }
  .ghost, .primary { height: 36px; padding: 0 16px; border-radius: 10px; font-weight: 500; display: inline-flex; align-items: center; gap: 6px; flex: none; }
  .ghost { background: transparent; border: 1px solid var(--line2); }
  .ghost.sm { height: 30px; padding: 0 12px; }
  .primary { background: var(--hero); color: var(--hero-text); border: 1px solid var(--hero); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .note { margin: 0; color: var(--muted); font-size: 13px; }
  .note.bad { color: var(--down); }
  .vtitle { display: flex; justify-content: space-between; gap: var(--s3); margin-top: var(--s2); color: var(--muted); }
  .vtitle b { color: var(--text); font-weight: 600; }
  .mono { font-family: var(--mono); font-size: 12px; color: var(--muted); }
  .choices { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; margin-top: var(--s1); }
  .choice { display: flex; align-items: center; gap: 10px; height: 40px; padding: 0 12px; margin: 0; border: 1px solid var(--line2); border-radius: 10px; color: var(--text-2); font-size: 13.5px; cursor: pointer; }
  .choice.on { border-color: var(--text); color: var(--text); background: #111; }
  .cl { flex: 1; }
  .grid2 { display: grid; grid-template-columns: 1fr 1fr; gap: var(--s3); align-items: end; }
  .grid2 > div { display: flex; flex-direction: column; gap: var(--s2); }
  .check { display: flex; align-items: center; gap: 10px; color: var(--text-2); font-size: 13.5px; }
  .check.end { height: 40px; }
  .list-tools { display: flex; gap: var(--s2); align-items: center; }
  .list-tools select { width: auto; height: 30px; margin-left: auto; }
  .entries { list-style: none; margin: 0; padding: 0; max-height: 38vh; overflow: auto; border: 1px solid var(--line); border-radius: 10px; }
  .entries li label { display: flex; align-items: center; gap: 10px; padding: 8px 12px; margin: 0; border-bottom: 1px solid var(--line); color: var(--text-2); font-size: 13.5px; cursor: pointer; }
  .entries li.gone { opacity: 0.4; }
  .idx { width: 28px; text-align: right; }
  .et { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row-btns { display: flex; justify-content: flex-end; gap: var(--s2); margin-top: var(--s3); }
</style>
