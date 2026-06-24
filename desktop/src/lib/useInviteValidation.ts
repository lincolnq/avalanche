import { createSignal } from "solid-js";
import { parseInviteUrl } from "../models/InviteToken";
import type { InviteInfo } from "../models/InviteToken";

export interface InviteValidation {
  error: () => string | null;
  setError: (msg: string | null) => void;
  isValidating: () => boolean;
  setIsValidating: (v: boolean) => void;
  validate: (raw: string) => Promise<void>;
}

/**
 * Shared invite-validation logic used by QRScannerView and InviteLinkEntryView.
 *
 * @param validateInvite  The `validateInvite` function from `useApp()`.
 * @param onValidated     Called with the resolved InviteInfo on success.
 * @param fallbackError   Error message text used when the thrown value is not an Error instance.
 */
export function useInviteValidation(
  validateInvite: (token: string) => Promise<InviteInfo>,
  onValidated: (info: InviteInfo, token: string) => void,
  fallbackError: string
): InviteValidation {
  const [error, setError] = createSignal<string | null>(null);
  const [isValidating, setIsValidating] = createSignal(false);

  async function validate(raw: string): Promise<void> {
    const trimmed = raw.trim();
    setError(null);
    if (!trimmed) return;
    setIsValidating(true);
    try {
      const token = parseInviteUrl(trimmed) ?? trimmed;
      const info = await validateInvite(token);
      onValidated(info, token);
    } catch (e) {
      setError(e instanceof Error ? e.message : fallbackError);
    } finally {
      setIsValidating(false);
    }
  }

  return { error, setError, isValidating, setIsValidating, validate };
}
