import { Show } from "solid-js";
import { Router, Route } from "@solidjs/router";
import { useApp } from "./state/AppContext";
import MainLayout from "./views/common/MainLayout";
import ChatsView from "./views/chats/ChatsView";
import NetworkView from "./views/network/NetworkView";
import OnboardingFlow from "./views/onboarding/OnboardingFlow";

export default function App() {
  const { store } = useApp();

  return (
    <Show when={!store.isOnboarding} fallback={<OnboardingFlow />}>
      <Router>
        <Route path="/" component={MainLayout}>
          <Route path="/" component={ChatsView} />
          <Route path="/chats" component={ChatsView} />
          <Route path="/chats/:conversationId" component={ChatsView} />
          <Route path="/network" component={NetworkView} />
        </Route>
      </Router>
    </Show>
  );
}
