// Recovery is established at signup on desktop: the account is created from a
// BIP39 phrase seed (RecoveryPhraseSetupView in onboarding), which uploads the
// recovery blob and makes the DID reproducible from the phrase. There is no
// post-hoc "set up recovery" step, so this banner has nothing to prompt and
// renders nothing — matching iOS, whose RecoveryKeyBanner is also inert.
export default function RecoveryKeyBanner() {
  return null;
}
