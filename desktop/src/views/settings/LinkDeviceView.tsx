import { createSignal, Match, onCleanup, Switch } from "solid-js";
import { FiArrowLeft, FiCheckCircle, FiCopy } from "solid-icons/fi";
import { useApp } from "../../state/AppContext";
import "./LinkDeviceView.css";

interface Props {
  onBack: () => void;
}

// Mirrors iOS LinkDeviceView (Settings): this signed-in device authorizes a new
// device to join the account. Show a code for the new device to enter (show
// mode) or enter the new device's code (paste mode); then poll until the
// provisioning bundle has been sealed and sent (linkSendBundle → "done").
type Phase =
  | { name: "choose" }
  | { name: "preparing" }
  | { name: "showing"; code: string }
  | { name: "entering" }
  | { name: "waiting" }
  | { name: "done" }
  | { name: "failed"; message: string };

export default function LinkDeviceView(props: Props) {
  const { linkShowCode, linkEnterCode, linkSendBundle } = useApp();
  const [phase, setPhase] = createSignal<Phase>({ name: "choose" });
  const [code, setCode] = createSignal("");

  let generation = 0;
  onCleanup(() => { generation++; });

  const showingPhase = () =>
    phase().name === "showing" ? (phase() as Extract<Phase, { name: "showing" }>) : null;
  const failedPhase = () =>
    phase().name === "failed" ? (phase() as Extract<Phase, { name: "failed" }>) : null;

  async function startShow() {
    const gen = ++generation;
    setPhase({ name: "preparing" });
    try {
      const c = await linkShowCode();
      if (gen !== generation) return;
      setPhase({ name: "showing", code: c });
      await linkSendBundle();
      if (gen !== generation) return;
      setPhase({ name: "done" });
    } catch (e) {
      if (gen !== generation) return;
      setPhase({ name: "failed", message: e instanceof Error ? e.message : "Linking failed" });
    }
  }

  async function submitCode() {
    const entered = code().trim();
    if (!entered) return;
    const gen = ++generation;
    setPhase({ name: "waiting" });
    try {
      await linkEnterCode(entered);
      if (gen !== generation) return;
      await linkSendBundle();
      if (gen !== generation) return;
      setPhase({ name: "done" });
    } catch (e) {
      if (gen !== generation) return;
      setPhase({ name: "failed", message: e instanceof Error ? e.message : "Linking failed" });
    }
  }

  function copyCode(c: string) {
    void navigator.clipboard?.writeText(c).catch(() => {});
  }

  function reset() {
    generation++;
    setCode("");
    setPhase({ name: "choose" });
  }

  return (
    <div class="link-device-panel">
      <header class="settings-subheader ld-header">
        <button class="back-btn" onClick={props.onBack}>
          <FiArrowLeft size={14} />Back
        </button>
        <h1>Link a device</h1>
      </header>

      <div class="ld-body">
        <Switch>
          <Match when={phase().name === "choose"}>
            <div class="ld-subtitle">
              Authorize another device to sign in to this account.
            </div>
            <div class="ld-actions">
              <button class="btn-primary ld-btn" onClick={() => void startShow()}>
                Show a code on this device
              </button>
              <button class="btn-secondary ld-btn" onClick={() => setPhase({ name: "entering" })}>
                Enter the new device's code
              </button>
            </div>
          </Match>

          <Match when={showingPhase()}>
            {(p) => (
              <>
                <div class="ld-subtitle">
                  On the new device: choose “Enter a code from my other device”, then type this code:
                </div>
                <div class="ld-code">{p().code}</div>
                <button class="btn-secondary ld-copy" onClick={() => copyCode(p().code)}>
                  <FiCopy size={14} />Copy code
                </button>
                <div class="ld-status"><span class="spinner" />Waiting for the new device…</div>
                <button class="back-btn ld-back" onClick={reset}>
                  <FiArrowLeft size={14} />Cancel
                </button>
              </>
            )}
          </Match>

          <Match when={phase().name === "entering"}>
            <div class="ld-subtitle">
              On the new device: choose “Show a code on this device”, then type that code here.
            </div>
            <input
              class="text-input ld-input"
              type="text"
              placeholder="av1…"
              value={code()}
              onInput={(e) => setCode(e.currentTarget.value)}
              onKeyDown={(e) => { if (e.key === "Enter") void submitCode(); }}
              autofocus
            />
            <button class="btn-primary ld-btn" disabled={!code().trim()} onClick={() => void submitCode()}>
              Link device
            </button>
            <button class="back-btn ld-back" onClick={reset}>
              <FiArrowLeft size={14} />Back
            </button>
          </Match>

          <Match when={phase().name === "preparing"}>
            <div class="ld-status"><span class="spinner" />Preparing…</div>
          </Match>

          <Match when={phase().name === "waiting"}>
            <div class="ld-status"><span class="spinner" />Linking…</div>
          </Match>

          <Match when={phase().name === "done"}>
            <div class="ld-status ld-done"><FiCheckCircle size={18} />Device linked.</div>
            <button class="btn-primary ld-btn" onClick={props.onBack}>Done</button>
          </Match>

          <Match when={failedPhase()}>
            {(p) => (
              <>
                <div class="ld-error">{p().message}</div>
                <button class="btn-primary ld-btn" onClick={reset}>Try again</button>
              </>
            )}
          </Match>
        </Switch>
      </div>
    </div>
  );
}
