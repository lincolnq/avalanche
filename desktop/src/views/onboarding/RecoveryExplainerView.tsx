import { createSignal } from "solid-js";
import { FiArrowLeft } from "solid-icons/fi";
import "./RecoveryExplainerView.css";

interface Props {
  onBack: () => void;
  onRecover: (phrase: string, serverUrl: string) => void;
}

const DEFAULT_SERVER = "http://localhost:3000";

/**
 * Recover an identity from a written recovery phrase + home server URL (mirrors
 * iOS RecoveryExplainerView, phrase path only — desktop has no passkey/PRF, so
 * the passkey option is omitted; see desktop/CLAUDE.md). Both inputs are needed:
 * the phrase derives the keys, the server URL lets the core recompute the DID
 * and locate the recovery blob.
 */
export default function RecoveryExplainerView(props: Props) {
  const [phrase, setPhrase] = createSignal("");
  const [serverUrl, setServerUrl] = createSignal(DEFAULT_SERVER);

  const canRecover = () => phrase().trim().length > 0 && serverUrl().trim().length > 0;

  return (
    <div class="recovery-explainer">
      <button class="back-btn recovery-explainer-back" onClick={props.onBack}>
        <FiArrowLeft size={14} />Back
      </button>

      <div class="recovery-explainer-body">
        <h1>Recover an identity</h1>
        <p class="recovery-explainer-hint">
          Enter the 12-word recovery phrase you wrote down and the home server you
          created this identity on.
        </p>

        <label class="recovery-explainer-field">
          <span>Recovery phrase</span>
          <textarea
            class="text-input recovery-explainer-phrase"
            value={phrase()}
            onInput={(e) => setPhrase(e.currentTarget.value)}
            rows={3}
            spellcheck={false}
            autocomplete="off"
            placeholder="word1 word2 word3 …"
          />
        </label>

        <label class="recovery-explainer-field">
          <span>Home server</span>
          <input
            class="text-input"
            value={serverUrl()}
            onInput={(e) => setServerUrl(e.currentTarget.value)}
            spellcheck={false}
            autocomplete="off"
            placeholder="https://server.example"
          />
        </label>

        <button
          class="btn-primary recovery-explainer-action"
          disabled={!canRecover()}
          onClick={() => props.onRecover(phrase().trim(), serverUrl().trim())}
        >
          Recover
        </button>
      </div>
    </div>
  );
}
