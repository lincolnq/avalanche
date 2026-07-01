import { createSignal, For } from "solid-js";
import { FiX } from "solid-icons/fi";
import "./RecipientTokenField.css";

interface Props {
  /** Selected recipient DIDs. */
  chips: string[];
  onAdd: (did: string) => void;
  onRemove: (did: string) => void;
  /** Resolve a chip's user-visible label. */
  displayName: (did: string) => string;
  placeholder?: string;
}

/**
 * Reusable chip ("token") input for selecting recipients. Presentational only —
 * the parent owns the chip list and dedup. Mirrors the iOS `RecipientTokenField`
 * behavior: typed text becomes a chip on Enter or comma (and on paste), trimmed.
 */
export default function RecipientTokenField(props: Props) {
  const [value, setValue] = createSignal("");

  function commit(raw: string) {
    const trimmed = raw.trim();
    if (!trimmed) return;
    if (!props.chips.includes(trimmed)) props.onAdd(trimmed);
    setValue("");
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter" || e.key === ",") {
      e.preventDefault();
      commit(value());
    } else if (e.key === "Backspace" && value() === "" && props.chips.length > 0) {
      // Backspace on an empty input removes the last chip.
      e.preventDefault();
      props.onRemove(props.chips[props.chips.length - 1]);
    }
  }

  function handlePaste(e: ClipboardEvent) {
    const text = e.clipboardData?.getData("text") ?? "";
    if (text.trim()) {
      e.preventDefault();
      commit(text);
    }
  }

  return (
    <div class="recipient-field">
      <For each={props.chips}>
        {(did) => (
          <span class="recipient-chip">
            <span class="recipient-chip-label">{props.displayName(did)}</span>
            <button
              class="recipient-chip-remove"
              onClick={() => props.onRemove(did)}
              aria-label="Remove recipient"
            >
              <FiX size={12} />
            </button>
          </span>
        )}
      </For>
      <input
        class="recipient-input"
        type="text"
        value={value()}
        placeholder={props.placeholder ?? "Type a name"}
        onInput={(e) => setValue(e.currentTarget.value)}
        onKeyDown={handleKeyDown}
        onPaste={handlePaste}
      />
    </div>
  );
}
