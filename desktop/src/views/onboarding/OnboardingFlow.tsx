import { createEffect, createSignal, Match, Switch } from "solid-js";
import { useApp } from "../../state/AppContext";
import type { InviteInfo } from "../../models/InviteToken";
import type { Account } from "../../models/Account";
import SplashView from "./SplashView";
import InviteLinkEntryView from "./InviteLinkEntryView";
import IdentityPickerView from "./IdentityPickerView";
import NewAccountView from "./NewAccountView";
import JoiningServerView from "./JoiningServerView";
import RecoveryPhraseSetupView from "./RecoveryPhraseSetupView";
import RecoveryExplainerView from "./RecoveryExplainerView";
import RecoveryConsoleView from "./RecoveryConsoleView";

type Screen =
  | { name: "splash" }
  | { name: "inviteLinkEntry" }
  | { name: "identityPicker"; inviteInfo: InviteInfo; inviteToken: string }
  | { name: "newAccount"; inviteInfo: InviteInfo; inviteToken: string }
  | { name: "recoveryPhraseSetup"; inviteInfo: InviteInfo; inviteToken: string; displayName: string }
  | { name: "joiningServer"; inviteInfo: InviteInfo; inviteToken: string; account: Account }
  | { name: "recoveryExplainer" }
  | { name: "recoveryConsole"; phrase: string; serverUrl: string };

export default function OnboardingFlow() {
  const { store, validateInvite, setPendingInviteToken } = useApp();

  // Back-stack: the current screen is always the last element.
  // The root entry ({ name: "splash" }) is never popped.
  const [stack, setStack] = createSignal<Screen[]>([{ name: "splash" }]);

  const current = (): Screen => stack()[stack().length - 1];

  /** Push a new screen onto the stack. */
  function navigate(s: Screen): void {
    setStack((prev) => [...prev, s]);
  }

  /** Pop the top screen. Never pops the root entry. */
  function goBack(): void {
    setStack((prev) => (prev.length > 1 ? prev.slice(0, -1) : prev));
  }

  // Deep links are handled centrally in AppContext (handleDeepLink), which sets
  // pendingInviteToken for invite tokens that need onboarding. The effect below
  // consumes that token; there is no separate listener here.
  createEffect(() => {
    const token = store.pendingInviteToken;
    if (!token) return;
    setPendingInviteToken(null);
    void validateInvite(token)
      .then((info) => {
        setStack([{ name: "splash" }, { name: "identityPicker", inviteInfo: info, inviteToken: token }]);
      })
      .catch(() => {
        setStack([{ name: "splash" }, { name: "inviteLinkEntry" }]);
      });
  });

  const identityPickerScreen = () =>
    current().name === "identityPicker"
      ? (current() as Extract<Screen, { name: "identityPicker" }>)
      : null;

  const newAccountScreen = () =>
    current().name === "newAccount"
      ? (current() as Extract<Screen, { name: "newAccount" }>)
      : null;

  const recoveryPhraseSetupScreen = () =>
    current().name === "recoveryPhraseSetup"
      ? (current() as Extract<Screen, { name: "recoveryPhraseSetup" }>)
      : null;

  const joiningServerScreen = () =>
    current().name === "joiningServer"
      ? (current() as Extract<Screen, { name: "joiningServer" }>)
      : null;

  const recoveryConsoleScreen = () =>
    current().name === "recoveryConsole"
      ? (current() as Extract<Screen, { name: "recoveryConsole" }>)
      : null;

  return (
    <Switch>
      <Match when={current().name === "splash"}>
        <SplashView
          onEnterLink={() => navigate({ name: "inviteLinkEntry" })}
          onRecover={() => navigate({ name: "recoveryExplainer" })}
        />
      </Match>

      <Match when={current().name === "inviteLinkEntry"}>
        <InviteLinkEntryView
          onValidated={(info, token) => navigate({ name: "identityPicker", inviteInfo: info, inviteToken: token })}
          onBack={goBack}
        />
      </Match>

      <Match when={identityPickerScreen()}>
        {(s) => (
          <IdentityPickerView
            inviteInfo={s().inviteInfo}
            onSelectAccount={(account) =>
              navigate({ name: "joiningServer", inviteInfo: s().inviteInfo, inviteToken: s().inviteToken, account })
            }
            onNewIdentity={() =>
              navigate({ name: "newAccount", inviteInfo: s().inviteInfo, inviteToken: s().inviteToken })
            }
            onBack={goBack}
          />
        )}
      </Match>

      <Match when={newAccountScreen()}>
        {(s) => (
          <NewAccountView
            inviteInfo={s().inviteInfo}
            showRecoverLink={true}
            onContinue={(displayName) =>
              navigate({ name: "recoveryPhraseSetup", inviteInfo: s().inviteInfo, inviteToken: s().inviteToken, displayName })
            }
            onRecover={() => navigate({ name: "recoveryExplainer" })}
            onBack={goBack}
          />
        )}
      </Match>

      <Match when={recoveryPhraseSetupScreen()}>
        {(s) => (
          <RecoveryPhraseSetupView
            inviteInfo={s().inviteInfo}
            token={s().inviteToken}
            displayName={s().displayName}
            onBack={goBack}
          />
        )}
      </Match>

      <Match when={joiningServerScreen()}>
        {(s) => (
          <JoiningServerView
            inviteInfo={s().inviteInfo}
            account={s().account}
            onBack={goBack}
          />
        )}
      </Match>

      <Match when={current().name === "recoveryExplainer"}>
        <RecoveryExplainerView
          onBack={goBack}
          onRecover={(phrase, serverUrl) => navigate({ name: "recoveryConsole", phrase, serverUrl })}
        />
      </Match>

      <Match when={recoveryConsoleScreen()}>
        {(s) => (
          <RecoveryConsoleView phrase={s().phrase} serverUrl={s().serverUrl} onBack={goBack} />
        )}
      </Match>
    </Switch>
  );
}
