import { Show } from "solid-js";
import { FiX } from "solid-icons/fi";
import { Router, Route } from "@solidjs/router";
import { useApp } from "./state/AppContext";
import MainLayout from "./views/common/MainLayout";
import ChatsView from "./views/chats/ChatsView";
import NetworkView from "./views/network/NetworkView";
import SettingsView from "./views/settings/SettingsView";
import OnboardingFlow from "./views/onboarding/OnboardingFlow";
import LaunchView from "./views/onboarding/LaunchView";
import { onEscape } from "./lib/onEscape";
import { startAutoUpdates } from "./state/updater";
import "./App.css";

export default function App() {
  const { store, cancelAddAccount } = useApp();
  // Background update checks (docs/63); a no-op in dev builds.
  startAutoUpdates();

  return (
    <Show
      when={!store.isOnboarding}
      fallback={
        // Hold a quiet launch screen while saved accounts open, so a returning
        // user never sees the welcome screen flash first.
        <Show when={!store.isLaunching} fallback={<LaunchView />}>
          <OnboardingFlow />
        </Show>
      }
    >
      <Router>
        <Route path="/" component={MainLayout}>
          <Route path="/" component={ChatsView} />
          <Route path="/chats" component={ChatsView} />
          <Route path="/chats/:conversationId" component={ChatsView} />
          <Route path="/network" component={NetworkView} />
          <Route path="/settings" component={SettingsView} />
        </Route>
      </Router>

      {/* "Sign in to another account": onboarding runs over the live session.
          On success, enterApp clears isAddingAccount and this unmounts, leaving
          the new account merged into the shared inbox. */}
      <Show when={store.isAddingAccount}>
        <AddAccountEscape onCancel={cancelAddAccount} />
        <div class="add-account-overlay">
          <div class="add-account-overlay-bar">
            <button class="back-btn" onClick={() => cancelAddAccount()} aria-label="Cancel">
              <FiX size={16} />
              Cancel
            </button>
          </div>
          <div class="add-account-overlay-content">
            <OnboardingFlow />
          </div>
        </div>
      </Show>
    </Show>
  );
}

// Esc cancels "Sign in to another account". Registered when the overlay
// mounts, below the onboarding screen's own Back, so inside the flow Esc steps
// back first and only cancels from the overlay's first screen.
function AddAccountEscape(props: { onCancel: () => void }) {
  onEscape(() => props.onCancel());
  return null;
}
