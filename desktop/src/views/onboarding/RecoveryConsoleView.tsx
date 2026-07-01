import { createSignal, For, onMount, Show } from "solid-js";
import { FiArrowLeft } from "solid-icons/fi";
import { useApp } from "../../state/AppContext";
import "./RecoveryConsoleView.css";

interface Props {
  phrase: string;
  serverUrl: string;
  onBack: () => void;
}

type Line = { text: string; kind: "info" | "ok" | "error" };

/**
 * Runs the recovery-phrase restore and streams progress (mirrors iOS
 * RecoveryConsoleView). On success, `recoverFromPhrase` flips the app out of
 * onboarding into the main UI, so no explicit navigation is needed.
 */
export default function RecoveryConsoleView(props: Props) {
  const { recoverFromPhrase } = useApp();
  const [lines, setLines] = createSignal<Line[]>([]);
  const [failed, setFailed] = createSignal(false);
  let started = false;

  const log = (text: string, kind: Line["kind"] = "info") =>
    setLines((prev) => [...prev, { text, kind }]);

  onMount(() => {
    if (started) return;
    started = true;
    void run();
  });

  async function run() {
    log(`Connecting to ${props.serverUrl}…`);
    log("Deriving your identity from the recovery phrase…");
    try {
      await recoverFromPhrase(props.phrase, props.serverUrl, "");
      log("Identity restored. Signing in…", "ok");
      // recoverFromPhrase entered the app; this view unmounts shortly.
    } catch (e) {
      log(e instanceof Error ? e.message : "Recovery failed", "error");
      log("Check that the server URL and recovery phrase are correct.");
      setFailed(true);
    }
  }

  return (
    <div class="recovery-console">
      <div class="recovery-console-header">
        <span>Recovering…</span>
      </div>
      <div class="recovery-console-log scrollbar-thin">
        <For each={lines()}>
          {(line) => <div class={`recovery-console-line recovery-console-${line.kind}`}>{line.text}</div>}
        </For>
      </div>
      <Show when={failed()}>
        <div class="recovery-console-actions">
          <button class="btn-secondary" onClick={props.onBack}>
            <FiArrowLeft size={14} />Try again
          </button>
        </div>
      </Show>
    </div>
  );
}
