import { createSignal, type JSX } from "solid-js";
import { useNavigate } from "@solidjs/router";
import { FiArrowLeft } from "solid-icons/fi";
import { useApp } from "../../state/AppContext";
import "./DevSettingsView.css";

interface Props {
  // When embedded inside the SettingsView hub, the hub supplies its own back
  // handler. When routed standalone, falls back to router navigation.
  onBack?: () => void;
}

export default function DevSettingsView(props: Props = {}): JSX.Element {
  const { store, logout, serverUrl, setServerUrl, closeToTray, setCloseToTray } = useApp();
  // Local draft of the server URL; committed (persisted) on Save.
  const [draftUrl, setDraftUrl] = createSignal(serverUrl());
  // useNavigate throws if rendered outside a Router — guard gracefully.
  let navigate: ReturnType<typeof useNavigate> | undefined;
  try {
    navigate = useNavigate();
  } catch {
    // rendered outside Router context (e.g. test), navigation is a no-op
  }

  function handleBack() {
    if (props.onBack) props.onBack();
    else navigate?.(-1);
  }

  function handleLogout() {
    logout();
    navigate?.("/");
  }

  const urlDirty = () => draftUrl().trim() !== serverUrl() && draftUrl().trim().length > 0;

  function saveUrl() {
    if (!urlDirty()) return;
    setServerUrl(draftUrl().trim());
  }

  return (
    <div class="dev-settings">
      <header class="dev-settings-header">
        <button class="back-btn" onClick={handleBack}>
          <FiArrowLeft size={14} />Back
        </button>
        <h1>Developer</h1>
      </header>

      <section class="dev-settings-section">
        <h2>Server</h2>
        <p class="dev-settings-hint">
          Home server used when creating a new account. Persisted across restarts.
        </p>
        <div class="dev-settings-url-row">
          <input
            class="text-input dev-settings-url-input"
            value={draftUrl()}
            onInput={(e) => setDraftUrl(e.currentTarget.value)}
            spellcheck={false}
            autocomplete="off"
            placeholder="http://localhost:3000"
          />
          <button class="btn-primary dev-settings-url-save" onClick={saveUrl} disabled={!urlDirty()}>
            Save
          </button>
        </div>
      </section>

      <section class="dev-settings-section">
        <h2>Window</h2>
        <label class="dev-settings-toggle-row">
          <div class="dev-settings-toggle-text">
            <span class="dev-settings-toggle-label">Keep running when closed</span>
            <span class="dev-settings-toggle-sub">
              Closing the window hides it to the system tray so messages and
              notifications keep arriving. Quit from the tray menu to exit fully.
            </span>
          </div>
          <input
            type="checkbox"
            class="dev-settings-toggle"
            checked={closeToTray()}
            onChange={(e) => setCloseToTray(e.currentTarget.checked)}
          />
        </label>
      </section>

      <section class="dev-settings-section">
        <h2>Session</h2>
        <p class="dev-settings-hint">
          {store.accounts.length === 0
            ? "No accounts signed in."
            : `${store.accounts.length} account${store.accounts.length > 1 ? "s" : ""} signed in.`}
        </p>
        <button class="btn-secondary" onClick={handleLogout}>
          Sign Out
        </button>
      </section>
    </div>
  );
}
