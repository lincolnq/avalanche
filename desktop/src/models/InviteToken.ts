// TypeScript never decodes the token bytes — the format is opaque to the
// frontend. We only extract the raw token string from URL path components and
// pass it to `commands.validateInvite(token)`.
//
// InviteInfo is now code-generated from Rust via tauri-specta → bindings.ts.

export type { InviteInfo } from "../bindings";

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
