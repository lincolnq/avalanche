import { Show } from "solid-js";
import { FiX } from "solid-icons/fi";
import { Router, Route } from "@solidjs/router";
import { useApp } from "./state/AppContext";
import MainLayout from "./views/common/MainLayout";
import ChatsView from "./views/chats/ChatsView";
import NetworkView from "./views/network/NetworkView";
import SettingsView from "./views/settings/SettingsView";
import OnboardingFlow from "./views/onboarding/OnboardingFlow";
import "./App.css";

export default function App() {
  const { store, cancelAddAccount } = useApp();

  return (
    <Show when={!store.isOnboarding} fallback={<OnboardingFlow />}>
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
