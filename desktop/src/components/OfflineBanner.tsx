import { createMemo, Show } from "solid-js";
import { useApp } from "../state/AppContext";
import "./OfflineBanner.css";

export default function OfflineBanner() {
  const { store, aggregateConnectionState, reconnectNow } = useApp();

  // Wrap in createMemo so the component re-evaluates reactively when
  // accounts change or the connection state transitions.  Reading these
  // in the raw function body would be untracked — the component would
  // never update after its initial render.
  const bannerText = createMemo((): string | null => {
    if (store.accounts.length === 0) return null;
    const state = aggregateConnectionState();
    if (state.type === "connected") return null;

    if (state.type === "reconnecting" && state.next_attempt_at_ms) {
      const secs = Math.max(
        0,
        Math.ceil((state.next_attempt_at_ms - Date.now()) / 1000)
      );
      return `Offline · retrying in ${secs}s`;
    }
    if (state.type === "reconnecting") return "Reconnecting…";
    if (state.type === "connecting") return "Connecting…";
    return "No connection";
  });

  return (
    <Show when={bannerText()}>
      {(text) => (
        <div class="offline-banner">
          <span>{text()}</span>
          <button class="offline-banner-retry" onClick={() => reconnectNow()}>
            Reconnect now
          </button>
        </div>
      )}
    </Show>
  );
}
