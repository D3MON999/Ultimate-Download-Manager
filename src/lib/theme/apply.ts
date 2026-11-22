// One place that turns the saved theme setting into the page's data-theme. Every window (the
// main one, new download, progress, video) calls this, so a theme follows everywhere.
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";

export type ThemePref = "dark" | "light" | "system" | "dock";

export function applyTheme(pref: string | undefined) {
  // ?theme= in the address wins (the demo pages and screenshots use it).
  const q = typeof location !== "undefined" ? new URLSearchParams(location.search).get("theme") : null;
  const t = q ?? pref ?? "dock";
  const name = t === "dock" ? "dock" : t === "light" || (t === "system" && matchMedia("(prefers-color-scheme: light)").matches) ? "light" : "dark";
  document.documentElement.dataset.theme = name;
  return name;
}
