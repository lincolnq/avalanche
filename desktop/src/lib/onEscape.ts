import { onCleanup } from "solid-js";

// Esc closes the topmost open dialog. Each modal registers its close handler
// for its lifetime; handlers form a stack so nested dialogs close one at a
// time. Listens in the capture phase so a focused input inside the dialog
// (e.g. the composer, which uses Esc to cancel an edit) doesn't also act on it.
const stack: Array<() => void> = [];
let installed = false;

function onKey(e: KeyboardEvent) {
  if (e.key !== "Escape" || stack.length === 0) return;
  e.preventDefault();
  e.stopPropagation();
  stack[stack.length - 1]();
}

export function onEscape(close: () => void): void {
  if (!installed) {
    window.addEventListener("keydown", onKey, true);
    installed = true;
  }
  const entry = () => close();
  stack.push(entry);
  onCleanup(() => {
    const i = stack.lastIndexOf(entry);
    if (i >= 0) stack.splice(i, 1);
  });
}
