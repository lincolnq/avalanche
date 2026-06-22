// TypeScript never decodes the token bytes — the format is opaque to the
// frontend. We only extract the raw token string from URL path components and
// pass it to `invoke('validate_invite', { token })`.

export interface InviteInfo {
  token: string;
  serverUrl: string;
  serverName: string;
  inviterDid?: string;
  inviterDisplayName?: string;
  postOnboardingRedirect?: string;
}

// Extract the raw token string from /i/<token> or /invite/<token> path.
// Returns null for any other URL shape.
export function parseInviteUrl(url: string): string | null {
  try {
    const parsed = new URL(url);
    const parts = parsed.pathname.split("/").filter((p) => p.length > 0);
    if (parts.length >= 2 && (parts[0] === "i" || parts[0] === "invite")) {
      return parts[1];
    }
    return null;
  } catch {
    return null;
  }
}
