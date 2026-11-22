// Ultimate Download Manager's download button over videos (a content script, in every page and
// frame). Our own design: a small pill at the video's edge that appears while the pointer is
// over the video (or it has the keyboard's focus) and fades when the pointer rests or leaves.
// Clicking it opens the app's Download video window for this page (an embedded player's own
// address, which yt-dlp understands), with the video's own address when it has a plain one. It can be dragged along the video's edge, and stays where
// it was put on that site. Its menu hides it on this video, keeps it off the site (the app's
// Options → General → Download panels lists those sites), or opens those settings. The app
// decides where it may show at all (the native host: integration on, this browser captured,
// the button on, the site not excluded).
(() => {
  // One button per page: a copy cut off from the extension (after an update) gives way to the
  // new one.
  const prev = globalThis.__udmVideoButton;
  if (prev && typeof prev === "object" && prev.alive()) return;
  if (prev && typeof prev.teardown === "function") prev.teardown();
  const ext = globalThis.browser ?? globalThis.chrome;

  // A video worth a button: big enough to be content, not a short muted looping clip (an
  // animated picture or a background). A sound player (<audio> with controls) gets one too.
  const MIN_W = 200;
  const MIN_H = 112;
  const MIN_AUDIO_H = 24;
  const CLIP_S = 15;
  // Shown this long after the pointer last moved over the video, kept while it is on the button.
  const IDLE_MS = 2600;
  const INSET = 10;
  const DRAG_PX = 4;
  // Our app icon (extension/icons/icon-48.png, as data: no extension file is exposed to pages).
  const LOGO = "data:image/png;base64,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";

  const t = (key, ...subs) => ext.i18n.getMessage(key, subs) || FALLBACK[key] || key;
  const FALLBACK = {
    video_download: "Download video",
    video_more: "More options",
    video_hide_here: "Hide on this video",
    video_settings: "Button settings...",
    video_sent: "Sent to Ultimate Download Manager",
    video_failed: "Ultimate Download Manager isn't installed or didn't start",
    video_off: "The download button is off on this site",
    video_drm: "Protected video: it can't be saved",
    video_unnamed: "Can't tell which video this is: open its own page and try there",
    video_drag: "Drag to move it along the video's edge",
  };
  // To the background. After the extension was updated or removed, this page's copy of the script
  // is cut off from it (calls throw at once): then it takes itself away.
  const send = (m) => {
    try {
      if (!ext.runtime?.id) throw new Error("gone");
      return ext.runtime.sendMessage(m).catch(() => ({ ok: false }));
    } catch {
      teardown();
      return Promise.resolve({ ok: false, gone: true });
    }
  };
  const host = location.hostname;
  // The address the app looks the video up by — the video's, not the page's. Most pages that play
  // a video are not the video's page: YouTube's home, a search, a channel or a playlist carries a
  // list in its address ("recommended", the results, the channel's videos), and yt-dlp reads such
  // an address as the list, so the app offered the whole feed instead of the video the button was
  // on. A watch page opened from a mix or a playlist has the same problem one level down
  // (…&list=…&index=…): its own page is watch?v=<id>, and nothing else.
  //
  // Null when the page plays a video but its DOM doesn't name it: YouTube's channel masthead player
  // has no video-id attribute, no link to the video inside the player and a blob: source. The page
  // itself never goes to the app then (a channel is a whole list to yt-dlp): the background asks
  // the player what it plays, in the page's own world (background.js playingVideo), and this
  // answer is only what it falls back on.
  function videoPage(v) {
    const watch = (id) =>
      /^[\w-]+$/.test(id || "") ? `https://www.youtube.com/watch?v=${id}` : null;
    try {
      const url = new URL(location.href);
      const name = url.hostname.replace(/^(www|m|music)\./, "");
      if (name === "youtu.be") {
        const id = url.pathname.split("/")[1];
        if (watch(id)) return `https://youtu.be/${id}`;
      }
      if (name === "youtube.com" || name === "youtube-nocookie.com") {
        // An embed's own page is the video's (…/embed/<id>).
        if (url.pathname.startsWith("/embed/")) {
          const id = watch(url.pathname.split("/")[2]);
          if (id) return id;
        }
        // A Short's or a live stream's own page (/shorts/<id>, /live/<id>). yt-dlp reads these as
        // the same one video as watch?v=<id>.
        const [, kind, pathId] = url.pathname.split("/");
        if (kind === "shorts" || kind === "live") {
          const id = watch(pathId);
          if (id) return id;
        }
        // The page is this video's own: a watch page, with or without a list on it.
        const own = watch(url.searchParams.get("v"));
        if (own && url.pathname.startsWith("/watch")) return own;
        const shown = watch(document.querySelector("ytd-watch-flexy")?.getAttribute("video-id"));
        if (shown && url.pathname.startsWith("/watch")) return shown;
        const player = watch(document.querySelector("#movie_player")?.getAttribute("video-id"));
        if (player) return player;
        // A player inside the card it belongs to (a feed, a search, a playlist): the card links to
        // the video. Only a container that names exactly one video counts — walking on into the
        // page's column would find some other card's link and send the wrong video, which is worse
        // than sending none.
        for (let node = v, i = 0; node && i < 12; i += 1, node = node.parentElement) {
          const ids = new Set();
          for (const a of node.querySelectorAll?.('a[href*="/watch?"], a[href*="youtu.be/"]') || []) {
            const link = new URL(a.href, location.href);
            const id = link.hostname.endsWith("youtu.be")
              ? link.pathname.split("/")[1]
              : link.searchParams.get("v");
            if (watch(id)) ids.add(id);
          }
          if (ids.size === 1) return watch([...ids][0]);
          if (ids.size > 1) break;
        }
        // A page that plays a video and names none: not the video's page, so not the app's.
        return null;
      }
    } catch {
      // Not an address this page can reason about: its own follows.
    }
    return location.href;
  }

  // The address this page first loaded at (`download`: a page that moves in place keeps its first
  // page's og:title).
  const landedAt = location.href;
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;

  // "Hide on this video": the video's address (players reuse one element for the next video),
  // else the element.
  const dismissed = new WeakSet();
  const dismissedSrc = new Set();
  const isDismissed = (v) => dismissed.has(v) || (v.currentSrc && dismissedSrc.has(v.currentSrc));
  let allowed = null; // asked once per page: may it show here?
  let place = { x: 1, y: 0 }; // where on the edge: fractions of the room, top right by default
  let chosen = false; // the user put it where it is (dragged it, on this site): theirs, never moved for a page's control
  let auto = { v: null, at: null, t: 0 }; // the spot found for this video, kept for a moment
  const AUTO_MS = 400;
  let video = null; // the video it is over
  let lastMove = 0;
  let pointer = { x: -1, y: -1 };
  let menuOpen = false;
  let dragging = null;
  let busy = false;
  let flashing = false;
  let raf = 0;
  let scan = 0;
  // Kept on screen while the keyboard is on it (not after a mouse click: that leaves focus too),
  // or on the video's player (its own controls).
  const keyboardOn = () =>
    Boolean(shadow.activeElement && shadow.activeElement.matches(":focus-visible")) || playerFocused();
  let focusedPlayer = null;
  const playerFocused = () =>
    Boolean(video && focusedPlayer && document.activeElement === focusedPlayer && focusedPlayer.contains(video));

  // ---- the button: a closed shadow tree the page's styles and scripts can't reach ----------
  const root = document.createElement("udm-video-button");
  // Each with !important: a page's own rules can't hide or move it.
  for (const [k, v] of Object.entries({
    all: "initial", position: "fixed", "z-index": "2147483647", left: "0", top: "0", display: "none",
    contain: "layout style", opacity: "1", visibility: "visible", "pointer-events": "auto",
  })) root.style.setProperty(k, v, "important");
  const shadow = root.attachShadow({ mode: "closed" });
  const CSS = `
:host { all: initial; }
* { box-sizing: border-box; }
/* Dark glass, the app's own look: it reads over bright and dark video alike, white type, the
   app's mark, a mint flash when sent. */
.pill {
  display: flex; align-items: stretch; height: 34px; border-radius: 10px;
  background: rgba(14, 14, 17, .82); color: #f4f4f5;
  border: 1px solid rgba(255, 255, 255, .14);
  box-shadow: 0 6px 20px rgba(0, 0, 0, .38), 0 1px 0 rgba(255, 255, 255, .06) inset;
  -webkit-backdrop-filter: blur(12px) saturate(140%); backdrop-filter: blur(12px) saturate(140%);
  font: 600 12.5px/1 "Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, Roboto, sans-serif;
  letter-spacing: .01em;
  opacity: 0; transform: translateY(-3px) scale(.98);
  transition: opacity .16s ease-out, transform .16s ease-out, background .12s, border-color .12s;
  user-select: none; -webkit-user-select: none; touch-action: none; white-space: nowrap;
}
.pill.shown { opacity: 1; transform: none; }
.pill:hover { background: rgba(22, 22, 26, .92); border-color: rgba(255, 255, 255, .24); }
.pill.dragging { cursor: grabbing; box-shadow: 0 12px 30px rgba(0, 0, 0, .5); transition: none; }
.pill.dragging .go { cursor: grabbing; }
.pill.ok { background: rgba(10, 60, 42, .9); border-color: #22c993; color: #c9fbe6; }
.pill.err { background: rgba(70, 14, 20, .9); border-color: #ff5d6c; color: #ffd5d9; }
button {
  /* all: unset resets box-sizing too, past the rule above: without it a menu item's 100 % width
     plus its padding ran past the menu's edge (#81). */
  all: unset; box-sizing: border-box; display: flex; align-items: center; gap: 8px;
  color: inherit; cursor: pointer; font: inherit;
}
.go { padding: 0 12px 0 6px; border-radius: 9px 0 0 9px; cursor: grab; }
.more { padding: 0 9px; border-left: 1px solid rgba(255, 255, 255, .12); border-radius: 0 9px 9px 0; }
.go:hover, .more:hover { background: rgba(255, 255, 255, .08); }
.go:active, .more:active { background: rgba(255, 255, 255, .14); }
button:focus-visible { outline: 2px solid #ffffff; outline-offset: -2px; }
.go img { width: 22px; height: 22px; flex: none; border-radius: 6px; }
.more svg { width: 12px; height: 12px; color: rgba(255, 255, 255, .7); }
.compact .label { display: none; }
.compact .go { padding: 0 6px; }
.menu {
  position: absolute; min-width: 220px; padding: 5px; border-radius: 12px;
  background: rgba(16, 16, 19, .96); color: #e9e9ec; border: 1px solid rgba(255, 255, 255, .12);
  box-shadow: 0 16px 40px rgba(0, 0, 0, .5);
  -webkit-backdrop-filter: blur(14px); backdrop-filter: blur(14px);
  font: 400 13px/1.2 "Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, Roboto, sans-serif;
  display: none;
}
.menu.open { display: block; }
.menu button { display: block; width: 100%; padding: 9px 12px; border-radius: 8px; cursor: default; }
.menu button:hover, .menu button:focus-visible { background: rgba(255, 255, 255, .09); color: #fff; outline: none; }
.menu button[data-act="download"] { font-weight: 600; color: #fff; }
.menu hr { border: 0; border-top: 1px solid rgba(255, 255, 255, .08); margin: 4px 4px; }@media (prefers-reduced-motion: reduce) { .pill { transition: none; transform: none; } }
`;
  // A constructed sheet: a page whose Content-Security-Policy refuses inline styles can't strip
  // it (Firefox applies the page's policy to what a content script adds).
  try {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(CSS);
    shadow.adoptedStyleSheets = [sheet];
  } catch {
    const style = document.createElement("style");
    style.textContent = CSS;
    shadow.appendChild(style);
  }
  shadow.innerHTML += `
<div class="pill" part="pill">
  <button class="go" type="button">
    <img alt="" draggable="false">
    <span class="label"></span>
  </button>
  <button class="more" type="button" aria-haspopup="menu" aria-expanded="false">
    <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.6"
      stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 4.5 6 7.5l3-3"/></svg>
  </button>
</div>
<div class="menu" role="menu">
  <button role="menuitem" data-act="download"></button>
  <hr>
  <button role="menuitem" data-act="hide"></button>
  <button role="menuitem" data-act="never"></button>
  <hr>
  <button role="menuitem" data-act="settings"></button>
</div>`;
  shadow.querySelector(".go img").src = LOGO;
  const pill = shadow.querySelector(".pill");
  const go = shadow.querySelector(".go");
  const label = shadow.querySelector(".label");
  const more = shadow.querySelector(".more");
  const menu = shadow.querySelector(".menu");
  const item = (act) => menu.querySelector(`[data-act="${act}"]`);
  pill.dir = ext.i18n.getMessage("@@bidi_dir") || "ltr";
  menu.dir = pill.dir;
  const idleLabel = t("video_download");
  label.textContent = idleLabel;
  go.title = `${idleLabel} — ${t("video_drag")}`;
  go.setAttribute("aria-label", idleLabel);
  more.title = t("video_more");
  more.setAttribute("aria-label", t("video_more"));
  item("download").textContent = idleLabel;
  item("hide").textContent = t("video_hide_here");
  item("never").textContent = ext.i18n.getMessage("video_never_site", [host]) || `Never on ${host}`;
  item("settings").textContent = t("video_settings");

  // ---- which video ----------------------------------------------------------------------------
  const videos = document.getElementsByTagName("video");
  const audios = document.getElementsByTagName("audio");

  // Videos inside open shadow roots (players built as web components), looked for again every
  // few seconds at most.
  let shadowed = [];
  let shadowedAt = 0;
  function shadowVideos() {
    if (Date.now() - shadowedAt < 3000) return shadowed;
    shadowedAt = Date.now();
    const found = [];
    const walk = (root, depth) => {
      if (depth > 4) return;
      const w = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
      for (let n = w.nextNode(), seen = 0; n && seen < 20000; n = w.nextNode(), seen++) {
        if (n.shadowRoot) {
          found.push(...n.shadowRoot.querySelectorAll("video"));
          walk(n.shadowRoot, depth + 1);
        }
      }
    };
    walk(document.documentElement, 0);
    shadowed = found;
    return found;
  }

  function eligible(v) {
    if (!v.isConnected || isDismissed(v)) return false;
    const r = v.getBoundingClientRect();
    const audio = v instanceof HTMLAudioElement;
    if (audio ? !v.controls || r.width < MIN_W || r.height < MIN_AUDIO_H : r.width < MIN_W || r.height < MIN_H) {
      return false;
    }
    // A short muted loop without controls: a moving picture, not something to save.
    if (v.muted && v.loop && !v.controls && !(v.duration > CLIP_S)) return false;
    const s = getComputedStyle(v);
    return s.visibility !== "hidden" && s.display !== "none" && Number(s.opacity) > 0.05;
  }

  // The topmost eligible video under (x, y): players lay their controls over the video, so the
  // video itself rarely gets the pointer's events.
  function videoAt(x, y) {
    let best = null;
    for (const v of [...videos, ...audios, ...shadowVideos()]) {
      const r = v.getBoundingClientRect();
      if (x < r.left || x > r.right || y < r.top || y > r.bottom || !eligible(v)) continue;
      if (!best || r.width * r.height < best.r.width * best.r.height) best = { v, r };
    }
    return best && best.v;
  }

  // ---- placing it -------------------------------------------------------------------------
  // `place` is where it sits in the room the video leaves around it: x and y from 0 to 1, one of
  // them 0 or 1 (on an edge).
  function rects() {
    const r = video.getBoundingClientRect();
    const w = pill.offsetWidth || 150;
    const h = pill.offsetHeight || 32;
    return { r, w, h, roomX: Math.max(0, r.width - w - 2 * INSET), roomY: Math.max(0, r.height - h - 2 * INSET) };
  }

  // Is a control of the page under this spot (a viewport rectangle)? What counts is something
  // on top of the video that the user clicks and that is much smaller than the video: a button,
  // a small link, a watch-later or menu button on a preview; a link or layer over the
  // whole video isn't one. On the video itself, and below it, is nothing to cover.
  function onControl(b, video, videoRect) {
    const area = (x) => Math.max(0, x.width) * Math.max(0, x.height);
    const small = (el) => area(el.getBoundingClientRect()) < area(videoRect) / 5;
    const points = [
      [(b.left + b.right) / 2, (b.top + b.bottom) / 2],
      [b.left + 4, b.top + 4],
      [b.right - 4, b.top + 4],
      [b.left + 4, b.bottom - 4],
      [b.right - 4, b.bottom - 4],
    ];
    for (const [x, y] of points) {
      for (const el of document.elementsFromPoint(x, y)) {
        if (el === root) continue;
        if (el === video || el.contains(video)) break;
        const control = el.closest("button, [role=button], a[href], input, select, [onclick]");
        if (control && !control.contains(video) && small(control)) return true;
        if (small(el) && getComputedStyle(el).cursor === "pointer") return true;
      }
    }
    return false;
  }

  // Where it goes now: the place the user gave it, else the default corner, or the next corner
  // free of the page's own controls.
  function spot(r, w, h, roomX, roomY) {
    if (chosen) return place;
    const now = Date.now();
    if (auto.v === video && now - auto.t < AUTO_MS) return auto.at;
    let at = place;
    for (const c of [place, { x: 0, y: 0 }, { x: 0, y: 1 }, { x: 1, y: 1 }]) {
      const left = r.left + INSET + c.x * roomX;
      const top = r.top + INSET + c.y * roomY;
      if (!onControl({ left, top, right: left + w, bottom: top + h }, video, r)) {
        at = c;
        break;
      }
    }
    auto = { v: video, at, t: now };
    return at;
  }

  function position() {
    if (!video) return;
    const { r, w, roomX, roomY } = rects();
    // A narrow video gets the icon only.
    pill.classList.toggle("compact", r.width < 360);
    const at = dragging ? dragging.at : spot(r, w, pill.offsetHeight || 32, roomX, roomY);
    const left = r.left + INSET + at.x * roomX;
    // In the part of the video on screen: one scrolled half out keeps its button in view.
    const h = pill.offsetHeight || 32;
    const visTop = Math.max(r.top, 0) + INSET;
    const visBottom = Math.min(r.bottom, innerHeight) - INSET - h;
    const top = Math.min(Math.max(r.top + INSET + at.y * roomY, visTop), Math.max(visTop, visBottom));
    root.style.setProperty("transform", `translate(${Math.round(left)}px, ${Math.round(top)}px)`, "important");
    if (menuOpen) placeMenu(top, left, w);
  }

  // The menu opens below the button, or above it near the bottom of the window; aligned with
  // whichever of its sides is nearer the video's middle.
  function placeMenu(top, left, w) {
    const h = menu.offsetHeight;
    const below = top + 36 + h < innerHeight;
    menu.style.top = below ? "36px" : `${-h - 4}px`;
    const { r } = rects();
    const rightHalf = left + w / 2 > r.left + r.width / 2;
    menu.style.left = rightHalf ? "" : "0";
    menu.style.right = rightHalf ? "0" : "";
  }

  // Snap a free spot (fractions, after a drag) to the nearest edge.
  function snap(at) {
    const d = { left: at.x, right: 1 - at.x, top: at.y, bottom: 1 - at.y };
    const edge = Object.keys(d).reduce((a, b) => (d[a] <= d[b] ? a : b));
    // Along the edge it stays where it was dropped, but close to a corner it goes into it.
    const c = (n) => (n < 0.08 ? 0 : n > 0.92 ? 1 : n);
    if (edge === "left") return { x: 0, y: c(at.y) };
    if (edge === "right") return { x: 1, y: c(at.y) };
    if (edge === "top") return { x: c(at.x), y: 0 };
    return { x: c(at.x), y: 1 };
  }

  // ---- showing and hiding ---------------------------------------------------------------------
  // Fullscreen (the page's own player, or the browser's): the button stays out of the picture.
  const fullscreen = () => !!(document.fullscreenElement || document.webkitFullscreenElement);

  function attach() {
    if (root.parentNode !== document.documentElement) document.documentElement.appendChild(root);
    return !fullscreen();
  }

  function show(v) {
    if (video !== v) {
      video = v;
      closeMenu();
    }
    if (!attach()) return hide(true);
    root.style.setProperty("display", "block", "important");
    position();
    if (!pill.classList.contains("shown")) requestAnimationFrame(() => pill.classList.add("shown"));
    if (!raf) raf = requestAnimationFrame(tick);
  }

  // `now`: at once and whatever has the focus (dismissed, the video gone).
  function hide(now) {
    if (!now && (menuOpen || dragging || keyboardOn())) return;
    if (now) {
      closeMenu();
      shadow.activeElement?.blur();
    }
    pill.classList.remove("shown");
    cancelAnimationFrame(raf);
    raf = 0;
    const was = video;
    setTimeout(() => {
      if (!pill.classList.contains("shown") && video === was) root.style.setProperty("display", "none", "important");
    }, now || reduced ? 0 : 180);
  }

  // While shown: follow the video (scrolling, layout) and fade when the pointer rests.
  function tick() {
    raf = 0;
    if (!video || !video.isConnected || !eligible(video)) {
      video = null;
      return hide(true);
    }
    const r = video.getBoundingClientRect();
    const over =
      pointer.x >= r.left && pointer.x <= r.right && pointer.y >= r.top && pointer.y <= r.bottom;
    const onButton = root.matches(":hover");
    if (!onButton && !menuOpen && !dragging && !keyboardOn() && (!over || Date.now() - lastMove > IDLE_MS)) {
      return hide();
    }
    position();
    raf = requestAnimationFrame(tick);
  }

  // At most one look per frame, however fast the pointer moves.
  function onMove(e) {
    pointer = { x: e.clientX, y: e.clientY };
    lastMove = Date.now();
    if (!scan) scan = requestAnimationFrame(look);
  }

  function look() {
    scan = 0;
    if (dragging || allowed === false) return;
    const v = videoAt(pointer.x, pointer.y);
    if (!v) return;
    if (allowed === null) return void ask().then(() => allowed && look());
    show(v);
  }

  async function ask() {
    if (allowed !== null) return;
    allowed = false; // until answered: one question per page
    const r = await send({ type: "panel" });
    allowed = Boolean(r && r.on);
    if (allowed) {
      try {
        const { positions } = await ext.storage.local.get({ positions: {} });
        if (positions[host]) {
          place = snap(positions[host]);
          chosen = true;
        }
      } catch {
        // Default spot.
      }
    }
  }

  // ---- acting ---------------------------------------------------------------------------------
  async function download() {
    if (busy || !video) return;
    busy = true;
    try {
      closeMenu();
      // Encrypted (DRM): no tool can save it; say so rather than send it.
      if (video.mediaKeys) return flash("err", t("video_drm"));
      const src = /^https?:/i.test(video.currentSrc || "") ? video.currentSrc : "";
      // A page that moved in place keeps its first page's og:title; its document title follows.
      const og = location.href === landedAt ? document.querySelector('meta[property="og:title"]')?.content : "";
      const title = (og || document.title || "").trim();
      // The middle of the video: where the background asks the page's own player what it plays.
      const box = video.getBoundingClientRect();
      const at = { x: box.left + box.width / 2, y: box.top + box.height / 2 };
      // Asked now, not kept: one <video> plays many videos at the same address (a feed's previews).
      const r = await send({ type: "video", src, title, url: videoPage(video), at });
      if (r.gone) return;
      flash(r.ok ? "ok" : "err", t(r.ok ? "video_sent" : r.why === "options" ? "video_off" : r.why === "unnamed" ? "video_unnamed" : "video_failed"));
    } finally {
      busy = false;
    }
  }

  // A short confirmation on the button itself, then back to normal.
  function flash(kind, text) {
    flashing = true;
    pill.classList.remove("ok", "err");
    pill.classList.add(kind);
    label.textContent = text;
    lastMove = Date.now();
    position();
    setTimeout(() => {
      flashing = false;
      pill.classList.remove(kind);
      label.textContent = idleLabel;
      if (video) position();
    }, 2200);
  }

  function openMenu() {
    menuOpen = true;
    menu.classList.add("open");
    more.setAttribute("aria-expanded", "true");
    position();
    menu.querySelector("button").focus();
  }

  function closeMenu() {
    if (!menuOpen) return;
    menuOpen = false;
    menu.classList.remove("open");
    more.setAttribute("aria-expanded", "false");
  }

  menu.addEventListener("click", (e) => {
    if (!e.isTrusted) return;
    const act = e.target.closest("[data-act]")?.dataset.act;
    if (act) run(act);
  });

  async function run(act) {
    if (act === "download") return download();
    closeMenu();
    if (act === "hide") {
      dismissed.add(video);
      if (video.currentSrc) dismissedSrc.add(video.currentSrc);
      video = null;
      return hide(true);
    }
    if (act === "never") {
      allowed = false;
      video = null;
      hide(true);
      return send({ type: "hide-site" });
    }
    if (act === "settings") send({ type: "options" });
  }

  more.addEventListener("click", () => (menuOpen ? closeMenu() : openMenu()));

  // Keys: Escape closes the menu, arrows move through it.
  shadow.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && menuOpen) {
      closeMenu();
      more.focus();
      e.stopPropagation();
    } else if (menuOpen && (e.key === "Enter" || e.key === " ") && menu.contains(shadow.activeElement)) {
      // The focused command, whatever the browser does with a key on a button.
      run(shadow.activeElement.dataset.act);
      e.preventDefault();
    } else if (menuOpen && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      const items = [...menu.querySelectorAll("button")];
      const i = items.indexOf(shadow.activeElement);
      items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length].focus();
      e.preventDefault();
    }
  });

  // The main part: a click downloads, a drag moves it (and is not a click).
  // Only the user's own clicks and keys (never events a page script makes up).
  go.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || !video || !e.isTrusted) return;
    const { r, roomX, roomY } = rects();
    dragging = {
      id: e.pointerId,
      startX: e.clientX,
      startY: e.clientY,
      // Where in the button it was taken.
      dx: e.clientX - (r.left + INSET + (chosen ? place : auto.at || place).x * roomX),
      dy: e.clientY - (r.top + INSET + (chosen ? place : auto.at || place).y * roomY),
      at: chosen ? place : auto.at || place,
      moved: false,
    };
    go.setPointerCapture(e.pointerId);
    e.preventDefault();
  });
  go.addEventListener("pointermove", (e) => {
    if (!dragging || e.pointerId !== dragging.id) return;
    if (!dragging.moved && Math.hypot(e.clientX - dragging.startX, e.clientY - dragging.startY) < DRAG_PX) return;
    if (!dragging.moved) {
      dragging.moved = true;
      closeMenu();
      pill.classList.add("dragging");
    }
    const { r, roomX, roomY } = rects();
    const c = (n) => Math.min(1, Math.max(0, n));
    dragging.at = {
      x: roomX ? c((e.clientX - dragging.dx - r.left - INSET) / roomX) : 0,
      y: roomY ? c((e.clientY - dragging.dy - r.top - INSET) / roomY) : 0,
    };
    position();
  });
  const endDrag = (e) => {
    if (!dragging || e.pointerId !== dragging.id) return;
    const d = dragging;
    dragging = null;
    pill.classList.remove("dragging");
    if (!d.moved) {
      if (e.type === "pointerup") download();
      return;
    }
    place = snap(d.at);
    chosen = true;
    position();
    lastMove = Date.now();
    // Remembered per site, not in a private window (it would keep the site's name), and the
    // 200 sites used last only.
    if (ext.extension?.inIncognitoContext) return;
    try {
      ext.storage.local
        .get({ positions: {} })
        .then(({ positions }) => {
          delete positions[host];
          positions[host] = place;
          const sites = Object.keys(positions);
          for (const s of sites.slice(0, Math.max(0, sites.length - 200))) delete positions[s];
          return ext.storage.local.set({ positions });
        })
        .catch(() => {});
    } catch {
      teardown();
    }
  };
  go.addEventListener("pointerup", endDrag);
  go.addEventListener("pointercancel", endDrag);
  // Keyboard: Enter / Space (a real button's click without a pointer).
  go.addEventListener("click", (e) => {
    if (e.detail === 0 && e.isTrusted) download();
  });

  // ---- wiring -----------------------------------------------------------------------------------
  // The extension updated or removed under this page: the button and its listeners go (a fresh
  // copy of this script comes with the new extension).
  function teardown() {
    cancelAnimationFrame(raf);
    cancelAnimationFrame(scan);
    removeEventListener("pointermove", onMove, true);
    removeEventListener("keydown", onTab, true);
    root.remove();
    allowed = false;
    video = null;
  }
  globalThis.__udmVideoButton = { alive: () => Boolean(ext.runtime?.id), teardown };
  addEventListener("pointermove", onMove, { capture: true, passive: true });
  addEventListener("pointerdown", (e) => {
    if (menuOpen && !e.composedPath().includes(root)) closeMenu();
  }, { capture: true, passive: true });
  // A video with the keyboard's focus, or a player around one (its controls), gets the button
  // too, kept while the focus stays there; Tab goes from there to the button.
  addEventListener("focusin", (e) => {
    if (!(e.target instanceof Element) || e.composedPath().includes(root)) return;
    const t = e.target;
    const v =
      t instanceof HTMLMediaElement ? t : [...t.querySelectorAll("video, audio")].find(eligible) || null;
    if (!v || !eligible(v)) return;
    focusedPlayer = t;
    ask().then(() => {
      if (!allowed) return;
      lastMove = Date.now();
      show(v);
    });
  }, true);
  // Tab on the focused player moves into the button first; Tab from the button goes back to
  // the player and on from there (its own controls next), as if the button sat at its start.
  let tabOn = 0;
  function onTab(e) {
    if (e.key !== "Tab" || e.shiftKey) return;
    // The Tab that just left the button: on from the player, as the browser goes.
    if (Date.now() - tabOn < 100) {
      tabOn = 0;
      return;
    }
    if (video && playerFocused() && root.style.getPropertyValue("display") === "block") {
      go.focus();
      e.preventDefault();
    }
  }
  addEventListener("keydown", onTab, true);
  go.addEventListener("keydown", (e) => {
    if (e.key === "Tab" && !e.shiftKey && focusedPlayer && focusedPlayer.isConnected) {
      tabOn = Date.now();
      focusedPlayer.focus();
    }
  });
  // Into fullscreen: gone at once. Back to the window: shown again over the video.
  const onFullscreen = () => (fullscreen() ? hide(true) : video && show(video));
  document.addEventListener("fullscreenchange", onFullscreen);
  document.addEventListener("webkitfullscreenchange", onFullscreen);
  addEventListener("blur", () => hide());
})();
