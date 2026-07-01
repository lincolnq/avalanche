import { disappearingLabel } from "../components/DisappearingMessagesPicker";

// Structured metadata app-core stores on a group system row (groups.rs
// persist_group_event). `event` is the kind_code integer, which also equals the
// message's `kind` field.
export interface GroupEventMeta {
  event: number;
  actor_did: string;
  target_did: string;
  target_emi: string;
  expiry_seconds: number;
  new_title: string;
}

/**
 * Parse the JSON metadata stored on a group system row into `GroupEventMeta`,
 * or `null` if absent/unparseable. The single source of truth for this shape —
 * both the timeline/preview renderer (`groupEventText`) and the display-name
 * warm pass (`AppContext.displayNameDidsToWarm`) read it through here so the
 * field names stay defined once.
 */
export function parseGroupEventMeta(
  metadata: string | undefined
): GroupEventMeta | null {
  if (!metadata) return null;
  try {
    return JSON.parse(metadata) as GroupEventMeta;
  } catch {
    return null;
  }
}

function eventName(
  did: string,
  accountId: string,
  resolveName: (did: string) => string,
  capitalized: boolean
): string {
  if (!did) return capitalized ? "Someone" : "someone";
  if (did === accountId) return capitalized ? "You" : "you";
  return resolveName(did);
}

/**
 * Human-readable line for a group system/metadata event (docs/03 §3.6),
 * mirroring the iOS `groupEventText`. Resolves actor/target DIDs to display
 * names ("You" for self) from the structured metadata, falling back to the
 * stored English summary (`fallbackBody`) when metadata is missing/unparseable.
 *
 * `kind_code` offsets group kinds by +1 (0 stays "normal chat message"), so the
 * `event` values below are 1–16.
 */
export function groupEventText(
  metadata: string | undefined,
  fallbackBody: string,
  accountId: string,
  resolveName: (did: string) => string
): string {
  const m = parseGroupEventMeta(metadata);
  if (!m) return fallbackBody;
  const actor = eventName(m.actor_did, accountId, resolveName, true);
  const target = eventName(m.target_did, accountId, resolveName, false);
  switch (m.event) {
    case 1:
      return `${actor} joined`;
    case 2:
      return `${actor} joined via invite link`;
    case 3:
      return `${actor} requested to join`;
    case 4:
      return `${actor} invited ${target}`;
    case 5:
      return `${actor} left the group`;
    case 6:
      return `${actor} removed ${target}`;
    case 7:
      return `${actor} approved ${target}'s request to join`;
    case 8:
      return `${actor} declined a join request`;
    case 9:
      return `${actor} declined the invitation`;
    case 10:
      return `${actor} cancelled their request to join`;
    case 11:
      return `${actor} made ${target} an admin`;
    case 12:
      return `${actor} removed ${target} as an admin`;
    case 13:
      return m.new_title
        ? `${actor} changed the group name to “${m.new_title}”`
        : `${actor} changed the group name`;
    case 14:
      return `${actor} changed the group description`;
    case 15:
      return m.expiry_seconds === 0
        ? `${actor} turned off disappearing messages`
        : `${actor} set disappearing messages to ${disappearingLabel(m.expiry_seconds)}`;
    case 16:
      return `${actor} changed the group settings`;
    default:
      return fallbackBody;
  }
}
