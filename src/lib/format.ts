const UNITS = ["B", "KB", "MB", "GB", "TB"];

/** 1536 → "1.5 KB" (1024-based, like Windows Explorer). */
export function bytes(n: number | null | undefined): string {
  if (n == null) return "—";
  let v = n;
  let u = 0;
  while (v >= 1024 && u < UNITS.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v >= 100 || u === 0 ? Math.round(v) : v.toFixed(1)} ${UNITS[u]}`;
}

export function speed(n: number): string {
  return `${bytes(n)}/s`;
}

/** Seconds → "2 h 05 m", "4 m 10 s", "12 s". */
export function eta(s: number | null | undefined): string {
  if (s == null) return "";
  if (s >= 3600) return `${Math.floor(s / 3600)} h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")} m`;
  if (s >= 60) return `${Math.floor(s / 60)} m ${String(s % 60).padStart(2, "0")} s`;
  return `${s} s`;
}

export function percent(done: number, size: number | null): number | null {
  if (!size) return null;
  return Math.min(100, (done / size) * 100);
}

export function when(unix: number | null): string {
  if (!unix) return "";
  return new Date(unix * 1000).toLocaleString(undefined, {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}
