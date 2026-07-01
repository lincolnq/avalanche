// TypeScript never decodes the token bytes — the format is opaque to the
// frontend. We only extract the raw token string from URL path components and
// pass it to `commands.validateInvite(token)`.
//
// InviteInfo is now code-generated from Rust via tauri-specta → bindings.ts.
//
// TODO(parity): when rebasing onto a main that includes `privacy_policy_url`
// on InviteInfo (upstream 44cfce2), regenerate bindings.ts and wire the field
// into IdentityPickerView / NewAccountView so users see the privacy link before
// creating an account. iOS and Android already consume this field.

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
