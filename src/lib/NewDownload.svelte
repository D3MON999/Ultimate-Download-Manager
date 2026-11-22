<script lang="ts">
  import { applyTheme } from "$lib/theme/apply";
  // "Download file info" (the classic layout): a download the browser handed over, waiting for
  // where it goes. Its own small window; the app's look.
  import Icon from "$lib/Icon.svelte";
  import { api, inTauri } from "$lib/api";
  import type { IconName } from "$lib/icons";
  import * as fmt from "$lib/format";
  import { open as pickFolder } from "@tauri-apps/plugin-dialog";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import Progress from "$lib/Progress.svelte";

  let { token }: { token: string } = $props();

  type Cat = { id: string; label: string; dir: string };
  type View = {
    url: string; name: string; size: number | null; mime: string | null; host: string; ext: string;
    category: string; categories: Cat[]; connections: number;
  };

  let v = $state<View | null>(null);
  let failed = $state("");
  let name = $state("");
  let dir = $state("");
  let category = $state("other");
  let skip = $state(false);
  let busy = $state(false);
  let error = $state("");
  let size = $state<number | null>(null);
  let dirTouched = false;
  /// Started: this window becomes its progress.
  let startedId = $state<number | null>(null);
  type PathInfo = { exists: boolean; free: number | null; problem: string | null; renamed: string };
  let pinfo = $state<PathInfo | null>(null);
  let whenExists = $state<"rename" | "replace">("rename");
  let pinfoTimer: ReturnType<typeof setTimeout> | undefined;
  // What's at the path now (debounced as it's typed): a file there, the space free.
  $effect(() => {
    const p = fullPath;
    const sz = size;
    clearTimeout(pinfoTimer);
    if (!inTauri || !name.trim() || !dir.trim()) {
      pinfo = null;
      return;
    }
    pinfoTimer = setTimeout(() => invoke<PathInfo>("path_info", { path: p, size: sz }).then((i) => (pinfo = i)).catch(() => {}), 200);
  });

  const ICONS: Record<string, IconName> = { video: "video", music: "music", docs: "doc", archives: "archive", programs: "app", images: "image" };
  const sep = $derived(dir.includes("/") && !dir.includes("\\") ? "/" : "\\");
  const fullPath = $derived(dir ? `${dir.replace(/[\\/]+$/, "")}${sep}${name}` : name);

  function setCategory(id: string) {
    category = id;
    // The folder follows the category until the person picks one.
    if (!dirTouched) dir = v?.categories.find((c) => c.id === id)?.dir ?? dir;
  }

  async function browse() {
    const p = await pickFolder({ directory: true, defaultPath: dir || undefined, title: "Save in" });
    if (typeof p === "string") {
      dir = p;
      dirTouched = true;
    }
  }

  async function answer(start: boolean) {
    error = "";
    if (!name.trim()) return (error = "Give the file a name.");
    if (!dir.trim()) return (error = "Choose a folder.");
    if (!inTauri) {
      if (start) startedId = 1;
      return;
    }
    busy = true;
    try {
      const target = pinfo?.exists && whenExists === "rename" ? `${dir.replace(/[\\/]+$/, "")}${sep}${pinfo.renamed}` : fullPath;
      const id = await invoke<number>("confirm_download", { token, path: target, category, start, skipType: skip, replace: !!pinfo?.exists && whenExists === "replace" });
      if (start) startedId = id;
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  function cancel() {
    if (inTauri) invoke("cancel_download", { token });
  }

  function onKey(e: KeyboardEvent) {
    if (startedId != null) return;
    if (e.key === "Escape") cancel();
    if (e.key === "Enter" && !busy && (e.target as HTMLElement)?.tagName !== "BUTTON") answer(true);
  }

  onMount(async () => {
    try {
      v = inTauri
        ? await invoke<View>("asked_download", { token })
        : {
            url: "https://fuckingfast.co/tse0e990adkj#Hozy_--_fitgirl-repacks.site_--_.part1.rar",
            name: "Hozy_--_fitgirl-repacks.site_--_.part1.rar", size: 2_097_152_000, mime: "application/x-rar", host: "fuckingfast.co", ext: "rar",
            category: "archives", connections: 8,
            categories: [
              { id: "video", label: "Video", dir: "D:\\Downloads\\Video" }, { id: "music", label: "Music", dir: "D:\\Downloads\\Music" },
              { id: "docs", label: "Documents", dir: "D:\\Downloads\\Documents" }, { id: "archives", label: "Compressed", dir: "D:\\Downloads\\Compressed" },
              { id: "programs", label: "Programs", dir: "D:\\Downloads\\Programs" }, { id: "images", label: "Images", dir: "D:\\Downloads\\Images" },
              { id: "other", label: "General", dir: "D:\\Downloads" },
            ],
          };
    } catch (e) {
      failed = String(e);
      return;
    }
    name = v.name;
    size = v.size;
    setCategory(v.category);
    if (inTauri) {
      api.settings().then((s) => {
        applyTheme(s.theme);
      });
      // The size and whether it can resume, from a quick look (the browser didn't always say).
      if (size == null) api.probe(v.url).then((info) => { if (size == null && info.size) size = info.size; }).catch(() => {});
    }
  });
</script>

<svelte:window onkeydown={onKey} />

{#if startedId != null}
  <Progress id={startedId} />
{:else}
<div class="nd">
  {#if failed}
    <div class="gone">
      <Icon name="done" size={22} />
      <p>{failed}</p>
      <button class="b" onclick={cancel}>Close</button>
    </div>
  {:else if v}
    <div class="head">
      <span class="ficon"><Icon name={ICONS[category] ?? "file"} size={20} /></span>
      <div class="what">
        <input class="nm" bind:value={name} spellcheck="false" aria-label="File name" />
        <span class="meta">
          {size != null ? fmt.bytes(size) : "Size unknown"} · {v.host}{v.mime ? ` · ${v.mime}` : ""}
        </span>
      </div>
    </div>

    <label class="lb" for="nd-url">Address</label>
    <div class="field ro">
      <input id="nd-url" value={v.url} readonly spellcheck="false" />
      <button class="ib" title="Copy the address" aria-label="Copy the address" onclick={() => navigator.clipboard?.writeText(v!.url)}><Icon name="link" size={14} /></button>
    </div>

    <div class="two">
      <div>
        <label class="lb" for="nd-cat">Category</label>
        <select id="nd-cat" value={category} onchange={(e) => setCategory(e.currentTarget.value)}>
          {#each v.categories as c (c.id)}<option value={c.id}>{c.label}</option>{/each}
        </select>
      </div>
      <div>
        <label class="lb" for="nd-con">Connections</label>
        <div class="field ro"><input id="nd-con" value="{v.connections} at once" readonly /></div>
      </div>
    </div>

    <label class="lb" for="nd-dir">Save as</label>
    <div class="field">
      <input id="nd-dir" value={fullPath} spellcheck="false"
        oninput={(e) => {
          const p = e.currentTarget.value;
          const i = Math.max(p.lastIndexOf("\\"), p.lastIndexOf("/"));
          dir = i >= 0 ? p.slice(0, i) : "";
          name = i >= 0 ? p.slice(i + 1) : p;
          dirTouched = true;
        }} />
      <button class="ib" title="Choose a folder" aria-label="Choose a folder" onclick={browse}><Icon name="folder" size={15} /></button>
    </div>

    {#if pinfo?.exists}
      <div class="exists" role="alert">
        <span>A file with this name is already there.</span>
        <label><input type="radio" bind:group={whenExists} value="rename" /> Keep both ({pinfo.renamed})</label>
        <label><input type="radio" bind:group={whenExists} value="replace" /> Replace it (the old one goes to the Recycle Bin)</label>
      </div>
    {/if}
    {#if pinfo?.problem}
      <p class="err" role="alert">{pinfo.problem}</p>
    {:else if pinfo?.free != null}
      <p class="free">{fmt.bytes(pinfo.free)} free on this drive</p>
    {/if}

    {#if v.ext}
      <label class="skip"><input type="checkbox" bind:checked={skip} /> Don't ask for .{v.ext} files (start them in their category's folder)</label>
    {/if}
    {#if error}<p class="err" role="alert">{error}</p>{/if}

    <div class="foot">
      <button class="b" onclick={cancel}>Cancel</button>
      <span class="grow"></span>
      <button class="b" disabled={busy} onclick={() => answer(false)}>Download later</button>
      <button class="b primary" disabled={busy} onclick={() => answer(true)}><Icon name="down" size={15} />Start download</button>
    </div>
  {/if}
</div>
{/if}

<style>
  :global(html, body) { overflow: hidden; }
  .nd { height: 100vh; display: flex; flex-direction: column; padding: 16px 20px 16px; background: var(--bg); color: var(--text); }
  .head { display: flex; align-items: center; gap: 12px; padding-bottom: 12px; border-bottom: 1px solid var(--line); }
  .ficon { width: 42px; height: 42px; border-radius: 10px; border: 1px solid var(--line2); display: grid; place-items: center; color: var(--text-2); flex: none; background: var(--card); }
  .what { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .nm { border: 1px solid transparent; background: transparent; color: var(--text); font-weight: 500; font-size: 14.5px; padding: 3px 6px; margin-left: -6px; border-radius: 6px; outline: none; text-overflow: ellipsis; }
  .nm:hover { border-color: var(--line2); }
  .nm:focus { border-color: var(--text-2); background: var(--field); }
  .meta { color: var(--muted); font-size: 12px; font-family: var(--mono); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .lb { display: block; color: var(--muted); font-size: 11.5px; margin: 12px 0 5px; letter-spacing: 0.02em; }
  .field { display: flex; align-items: center; height: 34px; border: 1px solid var(--line2); border-radius: 8px; background: var(--field); padding: 0 4px 0 10px; }
  .field:focus-within { border-color: var(--text-2); }
  .field input { flex: 1; min-width: 0; height: 100%; border: 0; background: transparent; color: var(--text); font-size: 12.5px; outline: none; font-family: var(--mono); }
  .field.ro input { color: var(--text-2); }
  .ib { width: 28px; height: 26px; border: 0; border-radius: 6px; background: transparent; color: var(--text-2); display: grid; place-items: center; flex: none; }
  .ib:hover { background: var(--card-hover); color: var(--text); }
  .two { display: grid; grid-template-columns: 1fr 1.4fr; gap: 12px; }
  select { width: 100%; height: 34px; padding: 0 10px; border: 1px solid var(--line2); border-radius: 8px; background: var(--field); color: var(--text); font: inherit; font-size: 13px; outline: none; }
  select:focus { border-color: var(--text-2); }
  .skip { display: flex; align-items: center; gap: 8px; margin-top: 12px; color: var(--text-2); font-size: 12.5px; }
  .skip input { width: 14px; height: 14px; accent-color: var(--text); margin: 0; }
  .err { margin: 8px 0 0; color: var(--down); font-size: 12.5px; }
  .free { margin: 6px 0 0; color: var(--muted); font-size: 12px; }
  .exists { margin-top: 10px; padding: 8px 10px; border: 1px solid var(--line2); border-radius: 8px; display: flex; flex-direction: column; gap: 4px; font-size: 12.5px; color: var(--text-2); }
  .exists span { color: var(--text); }
  .exists label { display: flex; gap: 8px; align-items: center; }
  .exists input { accent-color: var(--text); margin: 0; }
  .foot { margin-top: auto; padding-top: 14px; display: flex; gap: 8px; align-items: center; border-top: 1px solid var(--line); }
  .grow { flex: 1; }
  .b { height: 36px; padding: 0 16px; border-radius: 8px; border: 1px solid var(--line2); background: transparent; color: var(--text-2); font-size: 13px; display: inline-flex; align-items: center; gap: 7px; }
  .b:hover:not(:disabled) { border-color: var(--line3); color: var(--text); }
  .b.primary { background: var(--hero); color: var(--hero-text); border-color: var(--hero); font-weight: 500; }
  .b.primary:hover:not(:disabled) { color: var(--hero-text); filter: brightness(0.92); }
  .b:disabled { opacity: 0.5; }
  .gone { margin: auto; display: flex; flex-direction: column; align-items: center; gap: 10px; color: var(--text-2); text-align: center; }
</style>
