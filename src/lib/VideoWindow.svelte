<script lang="ts">
  import { applyTheme } from "$lib/theme/apply";
  // The video button's window: a small one of its own, there at once. The usual qualities are
  // ready to download straight away (yt-dlp picks the stream itself); the video's exact formats
  // and sizes replace them when the look-up ends. Download turns it into the progress window.
  import Icon from "$lib/Icon.svelte";
  import Progress from "$lib/Progress.svelte";
  import { api, inTauri, type Choice, type Lookup, type VideoOpts } from "$lib/api";
  import * as fmt from "$lib/format";
  import { open as pickFolder } from "@tauri-apps/plugin-dialog";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";

  let { page, title: givenTitle }: { page: string; title: string } = $props();

  /// What can be downloaded before the look-up ends (the same as a playlist's qualities).
  const PRESETS: Choice[] = [
    { label: "Best quality (video + sound)", selector: "bv*+ba/b", ext: "mp4", size: null, audio: false, holds_subs: true },
    { label: "1080p", selector: "bv*[height<=1080]+ba/b[height<=1080]", ext: "mp4", size: null, audio: false, holds_subs: true },
    { label: "720p", selector: "bv*[height<=720]+ba/b[height<=720]", ext: "mp4", size: null, audio: false, holds_subs: true },
    { label: "480p (smaller)", selector: "bv*[height<=480]+ba/b[height<=480]", ext: "mp4", size: null, audio: false, holds_subs: true },
    { label: "Audio only (MP3)", selector: "mp3:0", ext: "mp3", size: null, audio: true, holds_subs: false },
  ];

  // svelte-ignore state_referenced_locally
  let title = $state(givenTitle || "");
  let lookup = $state<Lookup | null>(null);
  let looking = $state(true);
  let lookupError = $state("");
  let pick = $state(0);
  let sub = $state("");
  let embed = $state(true);
  let dir = $state("");
  let dirTouched = $state(false);
  let busy = $state(false);
  let error = $state("");
  let startedId = $state<number | null>(null);
  let elapsed = $state(0);
  let more = $state(false);
  let from = $state("");
  let to = $state("");
  let sponsor = $state(false);
  let chapters = $state(false);
  let audioLang = $state("");
  let picked = $state<Set<number>>(new Set());
  const LANG_NAMES: Record<string, string> = { en: "English", hi: "Hindi", es: "Spanish", fr: "French", de: "German", pt: "Portuguese", ja: "Japanese", ko: "Korean", ru: "Russian", ar: "Arabic", it: "Italian", ta: "Tamil", te: "Telugu", bn: "Bengali", zh: "Chinese", tr: "Turkish", id: "Indonesian" };
  const langName = (c: string) => LANG_NAMES[c.split("-")[0]] ?? c;
  /// `1:20` or `80` → `1:20` (yt-dlp takes both); empty stays empty.
  const time = (t: string) => t.trim().replace(/[^\d:.]/g, "");
  const section = $derived(time(from) || time(to) ? `${time(from) || "0"}-${time(to) || "inf"}` : "");

  const choices = $derived(lookup?.kind === "video" ? lookup.choices : PRESETS);
  const choice = $derived(choices[pick] ?? choices[0]);
  const host = $derived.by(() => {
    try {
      return new URL(page).hostname.replace(/^www\./, "");
    } catch {
      return "";
    }
  });

  async function look() {
    looking = true;
    lookupError = "";
    const t0 = Date.now();
    const timer = setInterval(() => (elapsed = Math.round((Date.now() - t0) / 1000)), 250);
    try {
      const r = inTauri ? await api.videoLookup(page) : await demoLookup();
      // Keep the person's choice by kind: best stays best, audio stays audio.
      const wasAudio = choice?.audio;
      lookup = r;
      if (r.kind === "list") picked = new Set(r.entries.filter((e) => !e.unavailable).map((e) => e.index));
      if (r.kind === "video") {
        title = r.title || title;
        pick = Math.max(0, wasAudio ? r.choices.findIndex((c) => c.audio) : 0);
      }
    } catch (e) {
      lookupError = String(e);
    } finally {
      clearInterval(timer);
      looking = false;
    }
  }

  async function browse() {
    const p = await pickFolder({ directory: true, defaultPath: dir || undefined, title: "Save in" });
    if (typeof p === "string") {
      dir = p;
      dirTouched = true;
    }
  }

  async function download() {
    error = "";
    if (!inTauri) {
      startedId = 1;
      return;
    }
    busy = true;
    try {
      if (lookup?.kind === "list") {
        const entries = lookup.entries.filter((e) => !e.unavailable && picked.has(e.index)).map((e) => ({ url: e.url, title: e.title }));
        if (!entries.length) {
          error = "Choose at least one video.";
          return;
        }
        const quality = choice.audio ? "mp3" : choice.selector.includes("1080") ? "1080" : choice.selector.includes("720") ? "720" : choice.selector.includes("480") ? "480" : "best";
        const n = await api.addPlaylist(entries, quality, dirTouched ? dir : null);
        error = `${n} videos added to the list.`;
        setTimeout(close, 1200);
        return;
      }
      const s = lookup?.kind === "video" ? lookup.subs.find((t) => t.code === sub) : undefined;
      // A chosen sound language: that track when the site has it, else the usual one.
      const selector =
        audioLang && !choice.audio && choice.selector.includes("+ba")
          ? `${choice.selector.replace("+ba", `+ba[language^=${audioLang}]`)}/${choice.selector}`
          : audioLang && choice.audio && !choice.selector.includes(":")
            ? `ba[language^=${audioLang}]/${choice.selector}`
            : choice.selector;
      const opts: VideoOpts = {
        selector,
        ext: choice.ext,
        expected: choice.size,
        subs: s && !choice.audio ? [s.code] : [],
        automatic_subs: s?.automatic ?? false,
        embed_subs: embed && choice.holds_subs,
        section,
        sponsorblock: sponsor,
        chapters: chapters && !section,
      };
      startedId = await api.addVideo(page, title || page, opts, dirTouched ? dir : null);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  const close = () => (inTauri ? getCurrentWindow().close() : undefined);

  function onKey(e: KeyboardEvent) {
    if (startedId != null) return;
    if (e.key === "Escape") close();
    if (e.key === "Enter" && !busy && (e.target as HTMLElement)?.tagName !== "BUTTON") download();
  }

  onMount(() => {
    if (inTauri) {
      api.settings().then((s) => {
        applyTheme(s.theme);
        if (!dirTouched) dir = s.sort_by_type ? `${s.download_dir}\\Video` : s.download_dir;
      });
    } else {
      dir = "D:\\Downloads\\Video";
    }
    look();
  });

  // ---- sample data for the browser preview
  function demoLookup(): Promise<Lookup> {
    if (new URLSearchParams(location.search).has("loading")) return new Promise(() => {});
    return Promise.resolve({
      kind: "video",
      title: "Building a download manager in Rust — full walkthrough",
      duration: 1843,
      needs_ffmpeg: false,
      subs: [{ code: "en", name: "English", automatic: false }, { code: "hi", name: "Hindi", automatic: true }],
      choices: [
        { label: "2160p (4K) · webm", selector: "a", ext: "mkv", size: 2_410_000_000, audio: false, holds_subs: true },
        { label: "1440p · webm", selector: "b", ext: "mkv", size: 1_190_000_000, audio: false, holds_subs: true },
        { label: "1080p · mp4", selector: "c", ext: "mp4", size: 612_000_000, audio: false, holds_subs: true },
        { label: "720p · mp4", selector: "d", ext: "mp4", size: 318_000_000, audio: false, holds_subs: true },
        { label: "Audio only · m4a", selector: "e", ext: "m4a", size: 29_800_000, audio: true, holds_subs: false },
      ],
    });
  }
</script>

<svelte:window onkeydown={onKey} />

{#if startedId != null}
  <Progress id={startedId} />
{:else}
  <div class="vw">
    <div class="head">
      {#if lookup?.kind === "video" && lookup.thumbnail}
        <img class="thumb" src={lookup.thumbnail} alt="" referrerpolicy="no-referrer" />
      {:else}
        <span class="ficon"><Icon name="video" size={20} /></span>
      {/if}
      <div class="what">
        <b title={title}>{title || "Video"}</b>
        <span class="meta">{host}{lookup?.kind === "video" && lookup.duration ? ` · ${fmt.eta(Math.round(lookup.duration))}` : ""}{lookup?.kind === "list" ? ` · playlist, ${lookup.entries.length} videos` : ""}</span>
      </div>
    </div>

    {#if looking}
      <div class="loading" role="status">
        <div class="lbar"><i></i></div>
        <span>Reading the video's formats{elapsed >= 2 ? ` · ${elapsed} s` : ""}{elapsed >= 15 ? " · the first time, yt-dlp and FFmpeg install themselves" : ""}. You can download now: the qualities below work already.</span>
      </div>
    {:else if lookupError}
      <p class="note bad" role="alert">Couldn't read the exact formats ({lookupError}). The qualities below still work.</p>
    {/if}

    <div class="lb">{lookup?.kind === "list" ? "Quality for every video" : "Quality"}</div>
    <div class="choices" role="radiogroup" aria-label="Quality">
      {#each choices as c, i (c.selector + i)}
        <label class="choice" class:on={pick === i}>
          <input type="radio" name="q" value={i} bind:group={pick} />
          <Icon name={c.audio ? "music" : "video"} size={15} />
          <span class="cl">{c.label}</span>
          <span class="mono sz">{c.size ? `~${fmt.bytes(c.size)}` : ""}</span>
        </label>
      {/each}
    </div>

    {#if lookup?.kind === "video" && lookup.subs.length && !choice.audio}
      <div class="row">
        <label class="lb inline" for="vw-subs">Subtitles</label>
        <select id="vw-subs" bind:value={sub}>
          <option value="">None</option>
          {#each lookup.subs as s (s.code + s.automatic)}<option value={s.code}>{s.name}{s.automatic ? " (automatic)" : ""}</option>{/each}
        </select>
        {#if sub && choice.holds_subs}<label class="check"><input type="checkbox" bind:checked={embed} /> Inside the video</label>{/if}
      </div>
    {/if}

    {#if lookup?.kind === "list"}
      <div class="lb">Videos ({picked.size} of {lookup.entries.length})
        <button class="link" onclick={() => lookup?.kind === "list" && (picked = new Set(lookup.entries.filter((e) => !e.unavailable).map((e) => e.index)))}>All</button>
        <button class="link" onclick={() => (picked = new Set())}>None</button>
      </div>
      <div class="entries">
        {#each lookup.entries as e (e.index)}
          <label class="entry" class:off={e.unavailable}>
            <input type="checkbox" disabled={e.unavailable} checked={picked.has(e.index)}
              onchange={(ev) => { const s = new Set(picked); ev.currentTarget.checked ? s.add(e.index) : s.delete(e.index); picked = s; }} />
            <span class="et">{e.index}. {e.title}</span>
            <span class="mono sz">{e.duration ? fmt.eta(Math.round(e.duration)) : ""}</span>
          </label>
        {/each}
      </div>
    {:else}
      <button class="more" onclick={() => (more = !more)} aria-expanded={more}>{more ? "▾" : "▸"} More options{section || sponsor || chapters || audioLang ? " (on)" : ""}</button>
      {#if more}
        <div class="opts">
          <div class="orow">
            <span class="ol">Only a part</span>
            <input class="tin" bind:value={from} placeholder="from 1:20" aria-label="From" />
            <span class="muted">to</span>
            <input class="tin" bind:value={to} placeholder="to 3:45" aria-label="To" />
            <span class="muted small">empty: the whole video</span>
          </div>
          <label class="check"><input type="checkbox" bind:checked={sponsor} /> Cut out sponsor segments and "subscribe" reminders (SponsorBlock)</label>
          {#if lookup?.kind === "video" && lookup.chapters}
            <label class="check"><input type="checkbox" bind:checked={chapters} disabled={!!section} /> Also save each chapter as its own file</label>
          {/if}
          {#if lookup?.kind === "video" && lookup.audio_langs?.length}
            <div class="orow">
              <span class="ol">Sound language</span>
              <select bind:value={audioLang}>
                <option value="">The original</option>
                {#each lookup.audio_langs as l (l)}<option value={l}>{langName(l)} ({l})</option>{/each}
              </select>
            </div>
          {/if}
        </div>
      {/if}
    {/if}

    <div class="lb">Save in</div>
    <div class="field">
      <input value={dir} spellcheck="false" aria-label="Folder" oninput={(e) => { dir = e.currentTarget.value; dirTouched = true; }} />
      <button class="ib" title="Choose a folder" aria-label="Choose a folder" onclick={browse}><Icon name="folder" size={15} /></button>
    </div>

    {#if error}<p class="note bad" role="alert">{error}</p>{/if}

    <div class="foot">
      <button class="b" onclick={close}>Cancel</button>
      <span class="grow"></span>
      <button class="b primary" disabled={busy} onclick={download}><Icon name="down" size={15} />{lookup?.kind === "list" ? `Download ${lookup.entries.length} videos` : "Download"}</button>
    </div>
  </div>
{/if}

<style>
  :global(html, body) { overflow: hidden; }
  .vw { height: 100vh; display: flex; flex-direction: column; padding: 16px 20px 16px; background: var(--bg); color: var(--text); }
  .head { display: flex; align-items: center; gap: 12px; padding-bottom: 12px; border-bottom: 1px solid var(--line); }
  .ficon { width: 42px; height: 42px; border-radius: 10px; border: 1px solid var(--line2); display: grid; place-items: center; color: var(--text-2); flex: none; background: var(--card); }
  .what { min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .what b { font-size: 14.5px; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta { color: var(--muted); font-size: 12px; font-family: var(--mono); }
  .loading { display: flex; flex-direction: column; gap: 6px; margin-top: 12px; font-size: 12px; color: var(--muted); }
  .lbar { height: 4px; border-radius: 2px; background: var(--line2); overflow: hidden; position: relative; }
  .lbar i { position: absolute; top: 0; bottom: 0; width: 35%; background: var(--bar); border-radius: 2px; animation: slide 1.1s ease-in-out infinite; }
  @keyframes slide { 0% { left: -35%; } 100% { left: 100%; } }
  .note { margin: 10px 0 0; font-size: 12.5px; color: var(--muted); }
  .note.bad { color: var(--down); }
  .lb { color: var(--muted); font-size: 11.5px; margin: 14px 0 6px; letter-spacing: 0.02em; }
  .lb.inline { margin: 0; }
  .choices { display: flex; flex-direction: column; gap: 4px; overflow: auto; min-height: 0; max-height: 220px; }
  .choice { display: flex; align-items: center; gap: 10px; height: 36px; padding: 0 12px; border: 1px solid var(--line2); border-radius: 8px; color: var(--text-2); cursor: pointer; font-size: 13px; flex: none; }
  .choice:hover { border-color: var(--line3); }
  .choice.on { border-color: var(--text); color: var(--text); background: var(--card); }
  .choice input { position: absolute; opacity: 0; pointer-events: none; }
  .cl { flex: 1; }
  .sz { color: var(--muted); font-size: 12px; }
  .mono { font-family: var(--mono); }
  .row { display: flex; align-items: center; gap: 10px; margin-top: 12px; }
  select { height: 32px; padding: 0 10px; border: 1px solid var(--line2); border-radius: 8px; background: var(--field); color: var(--text); font: inherit; font-size: 12.5px; outline: none; }
  .check { display: inline-flex; align-items: center; gap: 6px; color: var(--text-2); font-size: 12.5px; }
  .check input { accent-color: var(--text); }
  .field { display: flex; align-items: center; height: 34px; border: 1px solid var(--line2); border-radius: 8px; background: var(--field); padding: 0 4px 0 10px; }
  .field:focus-within { border-color: var(--text-2); }
  .field input { flex: 1; min-width: 0; height: 100%; border: 0; background: transparent; color: var(--text); font: 12.5px var(--mono); outline: none; }
  .ib { width: 28px; height: 26px; border: 0; border-radius: 6px; background: transparent; color: var(--text-2); display: grid; place-items: center; }
  .ib:hover { background: var(--card-hover); color: var(--text); }
  .thumb { width: 96px; height: 54px; border-radius: 8px; object-fit: cover; flex: none; border: 1px solid var(--line2); background: var(--card); }
  .more { margin-top: 12px; align-self: flex-start; border: 0; background: none; color: var(--text-2); font-size: 12.5px; padding: 0; }
  .more:hover { color: var(--text); }
  .opts { margin-top: 8px; padding: 10px 12px; border: 1px solid var(--line2); border-radius: 10px; display: flex; flex-direction: column; gap: 8px; }
  .orow { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-size: 12.5px; }
  .ol { color: var(--text-2); min-width: 104px; }
  .tin { width: 92px; height: 30px; padding: 0 8px; border: 1px solid var(--line2); border-radius: 7px; background: var(--field); color: var(--text); font: 12.5px var(--mono); outline: none; }
  .tin:focus { border-color: var(--text-2); }
  .muted { color: var(--muted); }
  .small { font-size: 11.5px; }
  .link { border: 0; background: none; color: var(--text-2); text-decoration: underline; font-size: 11.5px; padding: 0 0 0 8px; }
  .entries { overflow: auto; max-height: 190px; display: flex; flex-direction: column; border: 1px solid var(--line2); border-radius: 8px; }
  .entry { display: flex; align-items: center; gap: 8px; padding: 6px 10px; border-bottom: 1px solid var(--line); font-size: 12.5px; color: var(--text-2); }
  .entry.off { opacity: 0.45; }
  .entry input { accent-color: var(--text); margin: 0; }
  .et { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .foot { margin-top: auto; padding-top: 14px; display: flex; gap: 8px; align-items: center; border-top: 1px solid var(--line); }
  .grow { flex: 1; }
  .b { height: 36px; padding: 0 16px; border-radius: 8px; border: 1px solid var(--line2); background: transparent; color: var(--text-2); font-size: 13px; display: inline-flex; align-items: center; gap: 7px; }
  .b:hover:not(:disabled) { border-color: var(--line3); color: var(--text); }
  .b.primary { background: var(--hero); color: var(--hero-text); border-color: var(--hero); font-weight: 500; }
  .b.primary:hover:not(:disabled) { color: var(--hero-text); filter: brightness(0.92); }
  .b:disabled { opacity: 0.5; }
</style>
