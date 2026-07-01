import { DeliveryStatus } from "../models/Message";

/**
 * Returns up to 2 uppercase initials from a display name.
 * Empty or whitespace-only names return "".
 */
export function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter((w) => w.length > 0)
    .slice(0, 2)
    .map((w) => w[0].toUpperCase())
    .join("");
}

/**
 * Encodes a contact invite token (base64url of `{s:serverUrl,d:inviterDid}`),
 * matching iOS `IdentityDetailView.makeInviteToken`. Single-char wire keys keep
 * the token short. The decode side lives in `AppContext`'s deep-link handler.
 */
export function makeInviteToken(serverUrl: string, inviterDid: string): string {
  const json = JSON.stringify({ s: serverUrl, d: inviterDid });
  return btoa(json).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/**
 * Builds the shareable contact URL for an identity, matching iOS
 * `IdentityDetailView.contactURL` (`https://go.theavalanche.net/i/<token>`).
 */
export function contactInviteUrl(serverUrl: string, inviterDid: string): string {
  return `https://go.theavalanche.net/i/${makeInviteToken(serverUrl, inviterDid)}`;
}

/**
 * Returns the hostname of `url`, or `fallback` if the URL cannot be parsed.
 */
export function displayHost(url: string, fallback: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return fallback;
  }
}

/**
 * Formats a unix-ms timestamp as a locale hour:minute string (e.g. "2:34 PM").
 * Used by MessageBubble timestamps.
 */
export function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/**
 * Formats a unix-ms timestamp as a relative string for the conversation list.
 * < 60s → "Just now", < 60m → "{m}m", < 24h → "{h}h", < 48h → "Yesterday",
 * else locale date string.
 */
export function formatRelative(ms: number): string {
  const diff = Date.now() - ms;
  const secs = Math.floor(diff / 1000);
  if (secs < 60) return "Just now";
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  if (hours < 48) return "Yesterday";
  return new Date(ms).toLocaleDateString();
}

/**
 * Maps DeliveryStatus to a numeric rank for forward-progression comparisons.
 * sending=0, sent=1, delivered=2, read=3.  `failed`(4) returns -1 — it is a
 * terminal error state, not "more advanced than read".  Callers must handle
 * `failed` separately rather than comparing by magnitude.
 */
export function deliveryRank(s: DeliveryStatus): number {
  switch (s) {
    case DeliveryStatus.sending:   return 0;
    case DeliveryStatus.sent:      return 1;
    case DeliveryStatus.delivered: return 2;
    case DeliveryStatus.read:      return 3;
    case DeliveryStatus.failed:    return -1;
  }
}

/**
 * A run of message text: plain when `href` is absent, a clickable link when set.
 */
export interface LinkSegment {
  text: string;
  href?: string;
}

const URL_REGEX = /\bhttps?:\/\/[^\s<]+/gi;

function countChar(s: string, c: string): number {
  let n = 0;
  for (const ch of s) if (ch === c) n++;
  return n;
}

/**
 * Trims trailing punctuation that NSDataDetector would exclude from a detected
 * link: sentence punctuation always, and an unbalanced closing bracket/quote.
 * A balanced paren is kept (e.g. `…/Foo_(bar)`), mirroring iOS link behavior.
 */
function trimTrailingPunct(url: string): string {
  let s = url;
  for (;;) {
    const ch = s[s.length - 1];
    if (!ch) break;
    if (".,;:!?".includes(ch)) {
      s = s.slice(0, -1);
      continue;
    }
    if (ch === ")" && countChar(s, "(") < countChar(s, ")")) {
      s = s.slice(0, -1);
      continue;
    }
    if (ch === "]" && countChar(s, "[") < countChar(s, "]")) {
      s = s.slice(0, -1);
      continue;
    }
    if (ch === '"' || ch === "'") {
      s = s.slice(0, -1);
      continue;
    }
    break;
  }
  return s;
}

/**
 * Splits a message body into plain-text and link segments. Detects http/https
 * URLs (mirrors iOS `MessageBubble.linkified` / `NSDataDetector .link`), trimming
 * trailing punctuation. Always returns at least one segment. The caller renders
 * `href` segments as `<a>` that route through `open_external` — never in-app.
 */
export function linkify(body: string): LinkSegment[] {
  const segments: LinkSegment[] = [];
  let lastIndex = 0;
  URL_REGEX.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = URL_REGEX.exec(body)) !== null) {
    const url = trimTrailingPunct(m[0]);
    const start = m.index;
    const end = start + url.length;
    if (start > lastIndex) segments.push({ text: body.slice(lastIndex, start) });
    segments.push({ text: url, href: url });
    lastIndex = end;
    // Resume scanning right after the trimmed URL so trimmed punctuation is
    // still emitted as plain text.
    URL_REGEX.lastIndex = end;
  }
  if (lastIndex < body.length) segments.push({ text: body.slice(lastIndex) });
  if (segments.length === 0) segments.push({ text: body });
  return segments;
}

/**
 * The first http/https URL in `body`, or null. Mirrors iOS `AppState.firstURL`
 * (used to decide which URL to fetch a link preview for). Trailing punctuation
 * is trimmed identically to `linkify`.
 */
export function firstUrl(body: string): string | null {
  URL_REGEX.lastIndex = 0;
  const m = URL_REGEX.exec(body);
  return m ? trimTrailingPunct(m[0]) : null;
}

/**
 * Chat-list placeholder for a caption-less attachment message: "Photo" for an
 * image, "Attachment" otherwise. Mirrors the iOS chat-list preview.
 */
export function attachmentPlaceholder(contentType: string | null | undefined): string {
  return contentType?.startsWith("image/") ? "Photo" : "Attachment";
}

/**
 * Human-readable byte size (e.g. "1.5 MB", "256 KB"), mirroring iOS's
 * `ByteCountFormatter(.file)` used on attachment chips.
 */
export function formatBytes(bytes: number): string {
  if (bytes < 1000) return `${bytes} bytes`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1000;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  return `${value.toFixed(1)} ${units[unit]}`;
}

/**
 * Deterministic DID→palette-index in 0..11.  Same DID always yields the same
 * index across calls and sessions — pure, no randomness.
 * Used by AccountAvatar to pick a CSS palette class (CSP forbids inline style).
 */
export function avatarColorIndex(did: string): number {
  let h = 0;
  for (let i = 0; i < did.length; i++) {
    h = (h * 31 + did.charCodeAt(i)) | 0;
  }
  return ((h % 12) + 12) % 12;
}
