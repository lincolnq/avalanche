import { createSignal, For, Show, onMount } from "solid-js";
import { FiArrowLeft } from "solid-icons/fi";
import { useApp } from "../../state/AppContext";
import type { InviteInfo } from "../../models/InviteToken";
import "./RecoveryPhraseSetupView.css";

interface Props {
  inviteInfo: InviteInfo;
  token: string;
  displayName: string;
  onBack: () => void;
}

type Stage = "loading" | "display" | "verify";

// Pick three distinct 1-based word positions in ascending order.
function pickQuizPositions(count: number): number[] {
  if (count < 3) return Array.from({ length: Math.max(count, 1) }, (_, i) => i + 1);
  const chosen = new Set<number>();
  while (chosen.size < 3) chosen.add(1 + Math.floor(Math.random() * count));
  return [...chosen].sort((a, b) => a - b);
}

/**
 * Signup-time recovery-phrase flow (mirrors iOS RecoveryPhraseSetupView).
 * Desktop has no passkey, so the BIP39 phrase IS the account's recovery
 * credential: its derived seed is passed to createAccount as the PRF output,
 * making the rotation key + DID reproducible from the phrase. The user must
 * write the phrase down and confirm three words before the account is created.
 */
export default function RecoveryPhraseSetupView(props: Props) {
  const app = useApp();
  const { generateRecoveryPhrase, createAccount } = app;

  const [stage, setStage] = createSignal<Stage>("loading");
  const [words, setWords] = createSignal<string[]>([]);
  const [quizPositions, setQuizPositions] = createSignal<number[]>([]);
  const [answers, setAnswers] = createSignal<Record<number, string>>({});
  const [error, setError] = createSignal<string | null>(null);
  const [creating, setCreating] = createSignal(false);

  onMount(() => {
    void (async () => {
      try {
        const phrase = await generateRecoveryPhrase();
        const w = phrase.split(/\s+/).filter(Boolean);
        setWords(w);
        setQuizPositions(pickQuizPositions(w.length));
        setStage("display");
      } catch (e) {
        setError(e instanceof Error ? e.message : "Couldn't generate a recovery phrase");
      }
    })();
  });

  function allCorrect(): boolean {
    return quizPositions().every((pos) => {
      const expected = words()[pos - 1]?.toLowerCase() ?? "";
      const got = (answers()[pos] ?? "").trim().toLowerCase();
      return expected === got;
    });
  }

  async function verifyAndCreate() {
    if (!allCorrect()) {
      setError("Those words don't match. Double-check what you wrote down.");
      return;
    }
    setCreating(true);
    setError(null);
    try {
      const seed = await app.service().recoveryPhraseToSeed(words().join(" "));
      await createAccount(
        props.inviteInfo.serverUrl,
        props.inviteInfo.serverName,
        props.displayName,
        props.token,
        seed
      );
      // createAccount flips isOnboarding = false → App swaps to the main UI.
    } catch (e) {
      setError(e instanceof Error ? e.message : "Account creation failed");
      setCreating(false);
    }
  }

  return (
    <div class="phrase-setup">
      <button class="back-btn phrase-setup-back" onClick={props.onBack}>
        <FiArrowLeft size={14} />Back
      </button>

      <div class="phrase-setup-body scrollbar-thin">
        <Show when={stage() === "loading"}>
          <div class="phrase-setup-loading">Generating your recovery phrase…</div>
        </Show>

        <Show when={stage() === "display"}>
          <h1>Write down your recovery phrase</h1>
          <p class="phrase-setup-hint">
            These 12 words and your home server are the only way to recover this
            identity. Store them somewhere safe — anyone with them can access your
            account.
          </p>
          <div class="phrase-setup-server">
            <span class="phrase-setup-server-label">HOME SERVER</span>
            <span class="phrase-setup-server-name">{props.inviteInfo.serverName}</span>
            <span class="phrase-setup-server-url">{props.inviteInfo.serverUrl}</span>
          </div>
          <ol class="phrase-setup-words">
            <For each={words()}>{(word) => <li class="phrase-setup-word">{word}</li>}</For>
          </ol>
          <button
            class="btn-primary phrase-setup-action"
            onClick={() => {
              setAnswers({});
              setError(null);
              setStage("verify");
            }}
          >
            I've written it down
          </button>
        </Show>

        <Show when={stage() === "verify"}>
          <h1>Confirm your recovery phrase</h1>
          <p class="phrase-setup-hint">Enter the following words from the phrase you wrote down.</p>
          <div class="phrase-setup-quiz">
            <For each={quizPositions()}>
              {(pos) => (
                <label class="phrase-setup-quiz-row">
                  <span>Word #{pos}</span>
                  <input
                    class="text-input"
                    value={answers()[pos] ?? ""}
                    onInput={(e) => setAnswers({ ...answers(), [pos]: e.currentTarget.value })}
                    spellcheck={false}
                    autocomplete="off"
                  />
                </label>
              )}
            </For>
          </div>
          <Show when={error()}>
            <p class="settings-error">{error()}</p>
          </Show>
          <div class="phrase-setup-verify-actions">
            <button class="btn-secondary" onClick={() => setStage("display")} disabled={creating()}>
              Show phrase again
            </button>
            <button class="btn-primary" onClick={() => void verifyAndCreate()} disabled={creating()}>
              {creating() ? "Creating…" : "Verify & Create"}
            </button>
          </div>
        </Show>

        <Show when={error() && stage() !== "verify"}>
          <p class="settings-error">{error()}</p>
        </Show>
      </div>
    </div>
  );
}
