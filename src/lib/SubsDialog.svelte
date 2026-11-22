<script lang="ts">
  import Icon from "$lib/Icon.svelte";
  import { api, type Sub } from "$lib/api";
  import * as fmt from "$lib/format";
  import { onMount } from "svelte";

  let { onclose, onsay }: { onclose: () => void; onsay: (text: string, bad?: boolean) => void } = $props();

  let subs = $state<Sub[]>([]);
  let url = $state("");
  let quality = $state("best");
  let every = $state(360);
  let latest = $state(0);
  let busy = $state(false);
  let error = $state("");
  let checking = $state<number | null>(null);

  const QUALITY: Record<string, string> = { best: "Best", "1080": "≤1080p", "720": "≤720p", "480": "≤480p", mp3: "MP3" };
  const EVERY: Record<number, string> = { 60: "every hour", 360: "every 6 hours", 720: "twice a day", 1440: "once a day" };

  async function load() {
    subs = await api.subs();
  }

  async function add() {
    if (!url.trim()) return;
    busy = true;
    error = "";
    try {
      const s = await api.addSub(url.trim(), quality, Number(every), Number(latest));
      onsay(`Subscribed to ${s.title}${latest ? `; its ${latest} newest are downloading` : ""}`);
      url = "";
      await load();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function checkNow(s: Sub) {
    checking = s.id;
    try {
      const n = await api.checkSub(s.id);
      onsay(n ? `${n} new from ${s.title}` : `Nothing new from ${s.title}`);
      await load();
    } catch (e) {
      onsay(String(e), true);
    } finally {
      checking = null;
    }
  }

  onMount(load);
</script>

<div class="scrim" onclick={onclose} role="presentation"></div>
<div class="dialog" role="dialog" aria-modal="true" aria-labelledby="sub-title">
  <div class="dialog-head">
    <h2 id="sub-title">Subscriptions</h2>
    <button class="x" onclick={onclose} aria-label="Close"><Icon name="close" size={16} /></button>
  </div>
  <p class="note">Follow a channel or playlist (YouTube and the other video sites). Ultimate Download Manager checks it and downloads each new video by itself.</p>

  {#each subs as s (s.id)}
    <div class="sub">
      <Icon name="video" size={16} />
      <div class="meta">
        <b title={s.url}>{s.title}</b>
        <span>{QUALITY[s.quality] ?? s.quality} · {EVERY[s.every_min] ?? `every ${s.every_min} min`} · {s.last_error ? "last check failed" : s.last_check ? `checked ${fmt.when(s.last_check)}` : "not checked yet"}</span>
      </div>
      <button class="ghost sm" onclick={() => checkNow(s)} disabled={checking === s.id}>{checking === s.id ? "Checking…" : "Check now"}</button>
      <button class="ghost sm" onclick={async () => { await api.removeSub(s.id); await load(); }} aria-label="Unsubscribe from {s.title}"><Icon name="trash" size={14} /></button>
    </div>
  {:else}
    <p class="note">No subscriptions yet.</p>
  {/each}

  <h3>Add</h3>
  <label for="surl">Channel or playlist address</label>
  <input id="surl" bind:value={url} placeholder="https://www.youtube.com/@channel/videos" spellcheck="false" autocomplete="off" onkeydown={(e) => e.key === "Enter" && add()} />
  <div class="grid3">
    <div>
      <label for="sq">Quality</label>
      <select id="sq" bind:value={quality}>
        {#each Object.entries(QUALITY) as [k, v] (k)}<option value={k}>{v}</option>{/each}
      </select>
    </div>
    <div>
      <label for="se">Check</label>
      <select id="se" bind:value={every}>
        {#each Object.entries(EVERY) as [k, v] (k)}<option value={Number(k)}>{v}</option>{/each}
      </select>
    </div>
    <div>
      <label for="sl">Download now</label>
      <select id="sl" bind:value={latest}>
        <option value={0}>Only new ones</option>
        <option value={1}>The newest 1</option>
        <option value={3}>The newest 3</option>
        <option value={5}>The newest 5</option>
      </select>
    </div>
  </div>
  {#if busy}<p class="note" role="status">Reading the channel…</p>{:else if error}<p class="note bad" role="alert">{error}</p>{/if}
  <div class="row-btns">
    <button class="ghost" onclick={onclose}>Close</button>
    <button class="primary" onclick={add} disabled={busy || !url.trim()}>Subscribe</button>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.72); backdrop-filter: blur(4px); }
  .dialog { position: fixed; left: 50%; top: 8vh; transform: translateX(-50%); width: min(640px, calc(100vw - 32px)); max-height: 84vh; overflow: auto; background: var(--card); border: 1px solid var(--line2); border-radius: var(--r-tile); box-shadow: 0 30px 80px rgba(0, 0, 0, 0.8); padding: var(--s5); display: flex; flex-direction: column; gap: var(--s2); }
  .dialog-head { display: flex; justify-content: space-between; align-items: center; }
  h2 { margin: 0; font: 600 20px var(--head); }
  h3 { margin: 10px 0 0; font: 600 15px var(--head); }
  .x { border: 0; background: transparent; color: var(--muted); width: 32px; height: 32px; border-radius: 8px; display: grid; place-items: center; }
  label { color: var(--muted); font-size: 12.5px; }
  input, select { width: 100%; height: 40px; padding: 0 12px; background: var(--field); border: 1px solid var(--line2); border-radius: 10px; outline: none; color: var(--text); font: inherit; }
  input:focus, select:focus { border-color: var(--text); }
  .grid3 { display: grid; grid-template-columns: 1fr 1fr 1fr; gap: var(--s3); }
  .grid3 > div { display: flex; flex-direction: column; gap: var(--s2); }
  .sub { display: flex; align-items: center; gap: 10px; padding: 10px 12px; border: 1px solid var(--line2); border-radius: 10px; color: var(--text-2); }
  .meta { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .meta b { color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta span { font-size: 12.5px; color: var(--muted); }
  .ghost, .primary { height: 36px; padding: 0 14px; border-radius: 10px; font-weight: 500; display: inline-flex; align-items: center; gap: 6px; flex: none; }
  .ghost { background: transparent; border: 1px solid var(--line2); }
  .ghost.sm { height: 30px; padding: 0 10px; }
  .primary { background: var(--hero); color: var(--hero-text); border: 1px solid var(--hero); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .note { margin: 0; color: var(--muted); font-size: 13px; }
  .note.bad { color: var(--down); }
  .row-btns { display: flex; justify-content: flex-end; gap: var(--s2); margin-top: var(--s3); }
</style>
