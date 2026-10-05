// Dev-only helpers for driving the running app from the command line
// (`desktop/scripts/devctl`, via the Rust debug bridge in
// `src-tauri/src/debug_bridge.rs`). Installed on `window.__av` only when
// `import.meta.env.DEV`, so production builds don't include them.
import type { AppContextValue } from "../state/types";
import { setUpdateStatusForDev, type UpdateStatus } from "../state/updater";

type Summary = {
  id: string;
  title: string;
  isGroup: boolean;
  isRequest: boolean;
  isBlocked: boolean;
  accountId: string;
};

export function installDebugHelpers(ctx: AppContextValue): void {
  const conversations = (): Summary[] =>
    ctx.store.conversations.map((c) => ({
      id: c.id,
      title: c.title,
      isGroup: c.isGroup,
      isRequest: c.isRequest,
      isBlocked: c.isBlocked,
      accountId: c.accountId,
    }));

  // Find a conversation by exact id, else by case-insensitive title substring.
  const find = (query: string): Summary | undefined => {
    const all = conversations();
    const q = query.toLowerCase();
    return all.find((c) => c.id === query) ?? all.find((c) => c.title.toLowerCase().includes(q));
  };

  // Wait until the DOM settles after an action, so a screenshot taken right
  // after reflects it.
  const settle = (ms = 300) => new Promise((r) => setTimeout(r, ms));

  const helpers = {
    /** Current route, selection, accounts, and the conversation list. */
    state: () => ({
      route: window.location.pathname,
      selectedConversationId: ctx.selectedConversationId(),
      accounts: ctx.store.accounts.map((a) => ({ id: a.id, displayName: a.displayName })),
      conversations: conversations(),
    }),
    conversations,
    /** Open a conversation by id or title substring (switches to Chats). */
    open: async (query: string) => {
      const c = find(query);
      if (!c) throw new Error(`no conversation matching "${query}"`);
      ctx.selectConversation(c.id);
      await settle();
      return c;
    },
    /** Go to a sidebar section: "chats" | "network" | "settings". */
    goto: async (section: string) => {
      const link = document.querySelector<HTMLElement>(`a[href="/${section}"]`);
      if (!link) throw new Error(`no sidebar link for "${section}"`);
      link.click();
      await settle();
      return window.location.pathname;
    },
    /** Click the first visible button/link/element whose text matches. */
    click: async (text: string) => {
      const want = text.trim().toLowerCase();
      const candidates = Array.from(
        document.querySelectorAll<HTMLElement>("button, a, [role=button], [role=tab], li, div"),
      ).filter((el) => el.offsetParent !== null);
      // Prefer exact matches on interactive elements, then any element.
      const exact = candidates.find(
        (el) => el.matches("button, a, [role=button], [role=tab]") && el.innerText.trim().toLowerCase() === want,
      );
      const loose = candidates.find((el) => el.innerText.trim().toLowerCase() === want);
      const target = exact ?? loose;
      if (!target) throw new Error(`nothing clickable with text "${text}"`);
      target.click();
      await settle();
      return target.tagName.toLowerCase();
    },
    /** Force the auto-update status, to see its UI (e.g. {kind:"ready",version:"0.7.0"}). */
    setUpdateStatus: (status: UpdateStatus) => setUpdateStatusForDev(status),
    /** Visible text of the page, or of the first element matching `selector`. */
    text: (selector?: string) => {
      const el = selector ? document.querySelector<HTMLElement>(selector) : document.body;
      return el ? el.innerText : null;
    },
  };

  (window as unknown as { __av: typeof helpers }).__av = helpers;
}
