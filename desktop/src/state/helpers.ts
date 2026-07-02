import { DeliveryStatus, type Message } from "../models/Message";
import {
  ServiceMode,
  type AvalancheService,
  type StoredMessageFfi,
  type ConversationSummaryFfi,
  type AttachmentFfi,
  type LinkPreviewFfi,
} from "../services/AvalancheService";
import { MockAvalancheService } from "../services/MockAvalancheService";
import { DevServerAvalancheService } from "../services/DevServerAvalancheService";

export function makeService(mode: ServiceMode, accountId = ""): AvalancheService {
  return mode === ServiceMode.Mock
    ? new MockAvalancheService()
    : new DevServerAvalancheService(accountId);
}

// A conversation summary is a group iff it carries a group title or its id uses
// the `group-` prefix (DM ids are `dm-<account>-<peer>`). Single source of truth
// for the group/DM split, used by both the name-warm pass and the row builder.
export function isGroupSummary(s: ConversationSummaryFfi): boolean {
  return s.groupTitle !== null || s.conversationId.startsWith("group-");
}

export function messageFromFfi(m: StoredMessageFfi): Message {
  return {
    id: m.id,
    conversationId: m.conversationId,
    senderAccountId: m.senderDid,
    body: m.body,
    sentAtMs: m.sentAtMs,
    editedAtMs: m.editedAtMs ?? undefined,
    readAtMs: m.readAtMs ?? undefined,
    deliveryStatus: (m.deliveryStatus >= 0 && m.deliveryStatus <= 4
      ? m.deliveryStatus
      : DeliveryStatus.sent) as DeliveryStatus,
    editCount: m.editCount,
    isDeleted: m.deleted,
    kind: m.kind,
    metadata: m.metadata ?? undefined,
    expireTimerSecs: m.expireTimerSecs,
    expireAtMs: m.expireAtMs ?? undefined,
    attachments: m.attachments,
    previews: m.previews,
  };
}

// Build a StoredMessageFfi row for service().saveMessage from the fields that
// vary, defaulting the rest. The single source of truth for the persisted-row
// shape, shared by the optimistic-send, retry, and incoming-message paths (T75)
// so they can't drift field-by-field. `expireAtMs` is always null on write —
// app-core's reaper computes the actual expiry on read.
export function buildStoredMessage(opts: {
  id: string;
  conversationId: string;
  senderDid: string;
  body: string;
  sentAtMs: number;
  deliveryStatus: DeliveryStatus;
  readAtMs?: number | null;
  editedAtMs?: number | null;
  editCount?: number;
  deleted?: boolean;
  kind?: number;
  metadata?: string | null;
  expireTimerSecs: number;
  attachments?: AttachmentFfi[];
  previews?: LinkPreviewFfi[];
}): StoredMessageFfi {
  return {
    id: opts.id,
    conversationId: opts.conversationId,
    senderDid: opts.senderDid,
    body: opts.body,
    sentAtMs: opts.sentAtMs,
    editedAtMs: opts.editedAtMs ?? null,
    readAtMs: opts.readAtMs ?? null,
    deliveryStatus: opts.deliveryStatus,
    editCount: opts.editCount ?? 0,
    deleted: opts.deleted ?? false,
    kind: opts.kind ?? 0,
    metadata: opts.metadata ?? null,
    expireTimerSecs: opts.expireTimerSecs,
    expireAtMs: null,
    attachments: opts.attachments ?? [],
    previews: opts.previews ?? [],
  };
}

export function recipientDidFromConvId(
  convId: string,
  accountId: string
): string | null {
  const prefix = `dm-${accountId}-`;
  if (convId.startsWith(prefix)) return convId.slice(prefix.length);
  return null;
}

// ── Deep-link parsing (T61) ────────────────────────────────────────────────────

// Parse a deep-link URL into (action, arg), accepting both the custom
// `avalanche://<action>/<arg>` scheme (what the desktop OS launches) and the
// universal-link form `https://go.theavalanche.net/<action>/<arg>` (iOS parity).
export function parseDeepLink(raw: string): { action: string; arg: string } | null {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return null;
  }
  let segments: string[];
  if (url.protocol === "avalanche:") {
    // avalanche://a/b puts the first segment in `host`; the triple-slash form
    // avalanche:///a/b puts everything in the path. Handle both.
    segments = [url.host, ...url.pathname.split("/")].filter(Boolean);
  } else if (url.host === "go.theavalanche.net") {
    segments = url.pathname.split("/").filter(Boolean);
  } else {
    return null;
  }
  if (segments.length < 2) return null;
  return { action: segments[0], arg: segments.slice(1).join("/") };
}

// Decode a base64url invite token ({s:serverUrl,d:inviterDid}) — the decode
// side of lib/format.makeInviteToken, matching iOS handleDeepLink.
export function decodeInviteToken(
  token: string
): { serverUrl: string; inviterDid: string | null } | null {
  try {
    const b64 = token.replace(/-/g, "+").replace(/_/g, "/");
    // Restore the padding makeInviteToken strips, so atob decodes reliably
    // regardless of webview base64 strictness.
    const padded = b64 + "=".repeat((4 - (b64.length % 4)) % 4);
    const obj = JSON.parse(atob(padded)) as { s?: unknown; d?: unknown };
    if (typeof obj.s !== "string") return null;
    return { serverUrl: obj.s, inviterDid: typeof obj.d === "string" ? obj.d : null };
  } catch {
    return null;
  }
}

export const trimSlashes = (s: string) => s.replace(/\/+$/, "");
