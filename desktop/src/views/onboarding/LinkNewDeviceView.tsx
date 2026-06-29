import { createSignal, Match, onCleanup, Switch } from "solid-js";
import { FiArrowLeft, FiCopy } from "solid-icons/fi";
import { useApp } from "../../state/AppContext";
import "./LinkNewDeviceView.css";

interface Props {
  onBack: () => void;
}

// Mirrors iOS LinkNewDeviceView: this (new) device joins an account already
// signed in on another device. Two directions — enter the other device's code
// (paste mode), or show a code for the other device to enter (show mode). Both
// then poll until the provisioning bundle arrives (deviceLinkComplete), which
// installs the account and flips the app out of onboarding (this view unmounts).
type Phase =
  | { name: "choose" }
  | { name: "preparing" }
  | { name: "showing"; code: string }
  | { name: "entering" }
  | { name: "waiting" }
  | { name: "failed"; message: string };

export default function LinkNewDeviceView(props: Props) {
  const { deviceLinkShowCode, deviceLinkEnterCode, deviceLinkComplete, deviceLinkCancel } = useApp();
  const [phase, setPhase] = createSignal<Phase>({ name: "choose" });
  const [code, setCode] = createSignal("");

  // Each attempt gets a generation number; reset/unmount bumps it so an in-flight
  // poll loop's late resolution can't write stale phase state.
  let generation = 0;

  onCleanup(() => {
    generation++;
    void deviceLinkCancel();
  });

  const showingPhase = () =>
    phase().name === "showing" ? (phase() as Extract<Phase, { name: "showing" }>) : null;
  const failedPhase = () =>
    phase().name === "failed" ? (phase() as Extract<Phase, { name: "failed" }>) : null;

  async function startShow() {
    const gen = ++generation;
    setPhase({ name: "preparing" });
    try {
      const c = await deviceLinkShowCode();
      if (gen !== generation) return;
      setPhase({ name: "showing", code: c });
      await deviceLinkComplete();
      // Success: deviceLinkComplete entered the app; this view unmounts.
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
      await deviceLinkEnterCode(entered);
      if (gen !== generation) return;
      await deviceLinkComplete();
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
    void deviceLinkCancel();
    setCode("");
    setPhase({ name: "choose" });
  }

  return (
    <div class="link-device">
      <div class="ld-title">Link this device</div>
      <Switch>
        <Match when={phase().name === "choose"}>
          <div class="ld-subtitle">
            Add this device to an account you're already signed in to on another device.
          </div>
          <div class="ld-actions">
            <button class="btn-primary ld-btn" onClick={() => setPhase({ name: "entering" })}>
              Enter a code from my other device
            </button>
            <button class="btn-secondary ld-btn" onClick={() => void startShow()}>
              Show a code on this device
            </button>
          </div>
          <button class="back-btn ld-back" onClick={props.onBack}>
            <FiArrowLeft size={14} />Back
          </button>
        </Match>

        <Match when={phase().name === "entering"}>
          <div class="ld-subtitle">
            On your other device: Settings → Link a device → Show a code, then type that code here.
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

        <Match when={showingPhase()}>
          {(p) => (
            <>
              <div class="ld-subtitle">
                On your other device: Settings → Link a device → Enter a code, then type this code:
              </div>
              <div class="ld-code">{p().code}</div>
              <button class="btn-secondary ld-copy" onClick={() => copyCode(p().code)}>
                <FiCopy size={14} />Copy code
              </button>
              <div class="ld-status"><span class="spinner" />Waiting for the other device…</div>
              <button class="back-btn ld-back" onClick={reset}>
                <FiArrowLeft size={14} />Cancel
              </button>
            </>
          )}
        </Match>

        <Match when={phase().name === "waiting"}>
          <div class="ld-status"><span class="spinner" />Linking…</div>
        </Match>

        <Match when={failedPhase()}>
          {(p) => (
            <>
              <div class="ld-error">{p().message}</div>
              <button class="btn-primary ld-btn" onClick={reset}>Try again</button>
              <button class="back-btn ld-back" onClick={props.onBack}>
                <FiArrowLeft size={14} />Back
              </button>
            </>
          )}
        </Match>
      </Switch>
    </div>
  );
}
