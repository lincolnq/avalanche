import { onCleanup } from "solid-js";

// Esc goes back one step. Anything that can be backed out of — a dialog, an
// onboarding screen's Back, a settings sub-screen — registers a handler for its
// lifetime; handlers form a stack, so the most recently mounted (topmost) one
// acts and nested things unwind one at a time. A handler may return false to
// decline (e.g. a screen at its root), letting the next one down handle it.
// Listens in the capture phase so a focused input inside the active surface
// (e.g. the composer, which uses Esc to cancel an edit) doesn't also act on it.
type Handler = () => boolean | void;
const stack: Handler[] = [];
let installed = false;

function onKey(e: KeyboardEvent) {
  if (e.key !== "Escape" || e.defaultPrevented) return;
  for (let i = stack.length - 1; i >= 0; i--) {
    if (stack[i]() !== false) {
      e.preventDefault();
      e.stopPropagation();
      return;
    }
  }
}

export function onEscape(handler: Handler): void {
  if (!installed) {
    window.addEventListener("keydown", onKey, true);
    installed = true;
  }
  const entry: Handler = () => handler();
  stack.push(entry);
  onCleanup(() => {
    const i = stack.lastIndexOf(entry);
    if (i >= 0) stack.splice(i, 1);
  });
}
