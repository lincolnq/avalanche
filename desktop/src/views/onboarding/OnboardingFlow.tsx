import { createEffect, createSignal, Match, onCleanup, onMount, Switch } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import { useApp } from "../../state/AppContext";
import type { InviteInfo } from "../../models/InviteToken";
import type { Account } from "../../models/Account";
import SplashView from "./SplashView";
import InviteLinkEntryView from "./InviteLinkEntryView";
import IdentityPickerView from "./IdentityPickerView";
import NewAccountView from "./NewAccountView";
import JoiningServerView from "./JoiningServerView";

type Screen =
  | { name: "splash" }
  | { name: "inviteLinkEntry" }
  | { name: "identityPicker"; inviteInfo: InviteInfo; inviteToken: string }
  | { name: "newAccount"; inviteInfo: InviteInfo; inviteToken: string }
  | { name: "joiningServer"; inviteInfo: InviteInfo; inviteToken: string; account: Account };

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

  // TODO: "actnet-deeplink" is NOT yet emitted by the backend — no deep-link
  // plugin is wired in src-tauri/src/lib.rs. This listener is a placeholder
  // for when the Rust side wires deep links. Likewise, setPendingInviteToken
  // has no producer yet; the createEffect below is also a forward-looking stub.
  onMount(() => {
    let unlisten: (() => void) | undefined;
    listen<string>("actnet-deeplink", (ev) => {
      void validateInvite(ev.payload)
        .then((info) => {
          // Deep-link success: root + identityPicker so Back returns to splash.
          setStack([{ name: "splash" }, { name: "identityPicker", inviteInfo: info, inviteToken: ev.payload }]);
        })
        .catch(() => {
          // Token invalid — land on link entry so the user can try manually.
          // Stack is root + inviteLinkEntry so Back returns to splash.
          setStack([{ name: "splash" }, { name: "inviteLinkEntry" }]);
        });
    })
      .then((fn) => { unlisten = fn; })
      .catch(() => { /* Tauri not available in pure browser mode */ });
    onCleanup(() => unlisten?.());
  });

  // Consume pendingInviteToken set by AppContext (e.g. from a URL opened before
  // the listener registered), then validate and navigate.
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

  const joiningServerScreen = () =>
    current().name === "joiningServer"
      ? (current() as Extract<Screen, { name: "joiningServer" }>)
      : null;

  return (
    <Switch>
      <Match when={current().name === "splash"}>
        <SplashView
          onEnterLink={() => navigate({ name: "inviteLinkEntry" })}
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
            token={s().inviteToken}
            showRecoverLink={true}
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
    </Switch>
  );
}
