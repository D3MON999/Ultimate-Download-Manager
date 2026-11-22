// Errors in plain words: what happened, what to do, and the one button that helps. The engine's
// own text stays available as the details.

export type Fix = "retry" | "address" | "check" | "redownload" | "settings" | null;
export type Friendly = { title: string; hint: string; fix: Fix; details: string };

const RULES: [RegExp, string, string, Fix][] = [
  [/needs you:|wants you to wait or solve a captcha/i, "This file host wants you", "Wait for its timer or solve its captcha in a browser window; the app takes the file from there.", null],
  [/site check needed|checks for a real browser|bot check|cf-mitigated/i, "The site wants a browser check", "Pass its check once in a browser window; the download then carries on.", "check"],
  [/web page instead of the file|sent a web page/i, "The site sent a web page, not the file", "It may want you signed in, or the link has expired. Pass a site check, or get a fresh link.", "check"],
  [/link has expired|refresh the download address|\b410\b/i, "The download link has expired", "Get a fresh link from the site (click the download there again), or paste one as a new address.", "address"],
  [/\b404\b|not found/i, "The file isn't on the server any more", "The link is probably old. Get a fresh link from the site.", "address"],
  [/\b401\b|\b403\b|forbidden|refused|no access/i, "The server refused the download", "The site may want you signed in, or to start the download from its page. Try a site check, or start it from your browser.", "check"],
  [/\b429\b|too many requests|limit for this|slow down/i, "The site is limiting downloads right now", "It tries again by itself after a wait. A reconnect command (Settings → Network) can switch your address.", "retry"],
  [/\b50[0-9]\b|service unavailable|bad gateway|server error|isn't responding/i, "The server is having trouble", "This is usually temporary; it tries again by itself.", "retry"],
  [/timed out|timeout|no answer in time/i, "The server stopped answering", "Your connection or the server is slow right now. Try again in a moment.", "retry"],
  [/connection (was )?(reset|closed|aborted|refused)|closed the connection|broken pipe|unexpected eof/i, "The connection dropped", "It picks up where it stopped when you try again.", "retry"],
  [/dns|resolve|no such host|name or service not known|lookup/i, "The site's name couldn't be found", "Check your internet connection or VPN, then try again.", "retry"],
  [/adapter|vpn/i, "Your chosen network is off", "Connect your VPN, or choose Any network in Settings → Network.", "settings"],
  [/not enough space|no space|disk full|too big for/i, "Not enough room on the drive", "Free some space, or choose another folder in the download's options.", null],
  [/checksum|doesn't match/i, "The file came down damaged", "Its checksum doesn't match. Download it again.", "redownload"],
  [/certificate|tls|ssl|handshake/i, "The site's security certificate wasn't accepted", "The site may be misconfigured, or something on your network is intercepting it.", "retry"],
  [/drive or network share|folder .* is not available|path is too long/i, "The save folder isn't available", "Reconnect the drive, or choose another folder.", null],
  [/password/i, "The archive needs a password", "Add its password in the download's options, or to Settings → After download.", null],
];

export function friendly(error: string | null | undefined): Friendly {
  const raw = (error ?? "").trim();
  const text = raw.replace(/^(Site check needed|Needs you): /, "");
  for (const [re, title, hint, fix] of RULES) {
    if (re.test(raw)) return { title, hint, fix, details: text };
  }
  return { title: text ? text.charAt(0).toUpperCase() + text.slice(1) : "The download stopped", hint: "Try again; if it keeps failing, the details below say what the server answered.", fix: "retry", details: text };
}
