<script lang="ts">
  // A stopped download's error in plain words, what to do, and the button that does it; the
  // server's own words under "Details".
  import Icon from "$lib/Icon.svelte";
  import { api, type Row } from "$lib/api";
  import { friendly } from "$lib/errors";

  let { row, onsay, onsettings }: { row: Row; onsay: (text: string, bad?: boolean) => void; onsettings?: () => void } = $props();

  const f = $derived(friendly(row.error));
  let address = $state("");
  let open = $state(false);

  async function act(p: Promise<unknown>, done?: string) {
    try {
      await p;
      if (done) onsay(done);
    } catch (e) {
      onsay(String(e), true);
    }
  }
</script>

<div class="eb" role="alert">
  <div class="top">
    <Icon name="stopped" size={16} />
    <div class="txt">
      <b>{f.title}</b>
      <span>{f.hint}</span>
    </div>
  </div>
  <div class="acts">
    {#if f.fix === "retry"}
      <button class="b primary" onclick={() => act(api.resume([row.id]), "Trying again")}><Icon name="retry" size={14} />Try again</button>
    {:else if f.fix === "check"}
      <button class="b primary" onclick={() => act(api.siteCheck(row.url), "Pass the check in the new window; the download carries on by itself")}><Icon name="link" size={14} />Pass site check</button>
      <button class="b" onclick={() => act(api.browserDownload(row.id))}><Icon name="down" size={14} />Download in a browser window</button>
    {:else if f.fix === "redownload"}
      <button class="b primary" onclick={() => act(api.redownload([row.id]), "Downloading again")}><Icon name="retry" size={14} />Download again</button>
    {:else if f.fix === "settings" && onsettings}
      <button class="b primary" onclick={onsettings}><Icon name="settings" size={14} />Open settings</button>
      <button class="b" onclick={() => act(api.resume([row.id]))}><Icon name="retry" size={14} />Try again</button>
    {/if}
    {#if f.fix === "address" || f.fix === "check"}
      {#if row.kind === "file"}
        <input class="addr" bind:value={address} placeholder="Paste a fresh link for the same file" spellcheck="false" />
        <button class="b" disabled={!address.trim()} onclick={() => act(api.refreshAddress(row.id, address), "Carrying on from the new link")}>Use it</button>
      {/if}
    {/if}
    {#if f.fix !== "retry" && f.fix !== "settings" && f.fix !== "redownload"}
      <button class="b" onclick={() => act(api.resume([row.id]))}><Icon name="retry" size={14} />Try again</button>
    {/if}
    {#if f.details && f.details !== f.title}
      <button class="link" onclick={() => (open = !open)}>{open ? "Hide details" : "Details"}</button>
    {/if}
  </div>
  {#if open}<p class="details mono">{f.details}</p>{/if}
</div>

<style>
  .eb { display: flex; flex-direction: column; gap: 10px; padding: 12px 14px; border: 1px solid color-mix(in srgb, var(--down) 35%, transparent); border-radius: 12px; background: color-mix(in srgb, var(--down) 6%, transparent); }
  .top { display: flex; gap: 10px; color: var(--down); }
  .txt { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .txt b { font-size: 14px; font-weight: 500; color: var(--text); }
  .txt span { font-size: 13px; color: var(--text-2); }
  .acts { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .b { height: 32px; padding: 0 12px; border-radius: 8px; border: 1px solid var(--line2); background: transparent; color: var(--text-2); font-size: 13px; display: inline-flex; align-items: center; gap: 6px; }
  .b:hover:not(:disabled) { border-color: var(--line3); color: var(--text); }
  .b.primary { background: var(--hero); color: var(--hero-text); border-color: var(--hero); font-weight: 500; }
  .b:disabled { opacity: 0.45; }
  .addr { flex: 1; min-width: 200px; height: 32px; padding: 0 10px; border: 1px solid var(--line2); border-radius: 8px; background: var(--field); color: var(--text); font: 12.5px var(--mono); outline: none; }
  .link { border: 0; background: none; color: var(--muted); text-decoration: underline; font-size: 12px; padding: 0; margin-left: auto; }
  .details { margin: 0; font-size: 12px; color: var(--muted); user-select: text; word-break: break-word; }
  .mono { font-family: var(--mono); }
</style>
