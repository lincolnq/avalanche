import { createSignal, createMemo, createEffect, on, For, Show, onCleanup, onMount } from "solid-js";
import { FiEdit, FiSearch, FiX } from "solid-icons/fi";
import { useApp } from "../../state/AppContext";
import ConversationRow from "../../components/ConversationRow";
import AccountAvatar from "../../components/AccountAvatar";
import type { Account } from "../../models";
import RecoveryKeyBanner from "../../components/RecoveryKeyBanner";
import OfflineBanner from "../../components/OfflineBanner";
import NewConversationView from "../../components/NewConversationView";
import ConversationView from "./ConversationView";
import "./ChatsView.css";

const isMac = navigator.platform.toUpperCase().includes("MAC");

export default function ChatsView() {
  const {
    store,
    loadMessagesFromStore,
    unreadCount,
    selectedConversationId,
    selectConversation,
    selectedChatsAccountTab,
    setSelectedChatsAccountTab,
  } = useApp();
  const [showNew, setShowNew] = createSignal(false);
  // Conversation search (iOS ConversationSearchView, docs/37): client-side, by
  // title, across all accounts. On Desktop it's a field atop the chat list.
  const [query, setQuery] = createSignal("");
  let searchRef: HTMLInputElement | undefined;

  const selected = () =>
    store.conversations.find((c) => c.id === selectedConversationId()) ?? null;

  const totalUnread = createMemo(() =>
    store.conversations.reduce((sum, c) => sum + unreadCount(c), 0)
  );

  // Sort by recency at render (parity with iOS/Android), not just at store-load:
  // rows appended by findOrCreateDM/GroupConversation and in-place title updates
  // don't re-sort the store, so the render must own the ordering.
  const sortedConversations = createMemo(() =>
    [...store.conversations].sort(
      (a, b) => (b.lastMessageDate ?? 0) - (a.lastMessageDate ?? 0)
    )
  );
  // Account tabs (docs/37, iOS ChatsView.accountTabStrip): only with more
  // than one identity. The effective tab is derived at read time (explicit
  // choice if it names a live account, else the first) so the first render is
  // already filtered.
  const showsAccountTabs = () => store.accounts.length > 1;
  const selectedAccountTab = (): string | null => {
    if (!showsAccountTabs()) return null;
    const sel = selectedChatsAccountTab();
    return sel && store.accounts.some((a) => a.id === sel) ? sel : store.accounts[0]?.id ?? null;
  };
  const accountLabel = (a: Account) => a.displayName || a.servers[0]?.displayHost || "Account";
  const unreadFor = (accountId: string) =>
    store.conversations
      .filter((c) => c.accountId === accountId)
      .reduce((sum, c) => sum + unreadCount(c), 0);

  // Search spans every account (iOS's Search tab ignores the account tabs);
  // otherwise the list is the selected tab's conversations.
  const visibleConversations = createMemo(() => {
    const q = query().trim().toLocaleLowerCase();
    const all = sortedConversations();
    if (q) return all.filter((c) => c.title.toLocaleLowerCase().includes(q));
    const tab = selectedAccountTab();
    return tab ? all.filter((c) => c.accountId === tab) : all;
  });

  // A conversation opened from elsewhere (deep link, notification, search
  // across accounts) switches to its account's tab so the selection is visible.
  createEffect(
    on(selectedConversationId, (id) => {
      if (!id || !showsAccountTabs()) return;
      const conv = store.conversations.find((c) => c.id === id);
      if (conv && conv.accountId !== selectedAccountTab()) setSelectedChatsAccountTab(conv.accountId);
    }),
  );

  function open(id: string, focusComposer: boolean) {
    const conv = store.conversations.find((c) => c.id === id);
    if (!conv) return;
    selectConversation(id);
    loadMessagesFromStore(id, conv.accountId);
    if (focusComposer) {
      setTimeout(() => document.querySelector<HTMLTextAreaElement>(".compose-input")?.focus(), 0);
    }
  }

  function clearSearch() {
    setQuery("");
    searchRef?.blur();
  }

  function onSearchKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      clearSearch();
    } else if ((e.key === "Enter" || e.key === "ArrowDown") && visibleConversations().length > 0) {
      e.preventDefault();
      open(visibleConversations()[0].id, true);
      setQuery("");
    }
  }

  // Desktop keyboard shortcuts (no mobile equivalent): Cmd/Ctrl+K or F search,
  // Cmd/Ctrl+N new message, Alt+Cmd/Ctrl+Up/Down previous/next conversation
  // (Signal Desktop's binding), or plain Alt+Up/Down outside text fields (in a
  // text field that moves the caret by paragraph on macOS).
  function onGlobalKey(e: KeyboardEvent) {
    const mod = e.metaKey || e.ctrlKey;
    if (mod && !e.shiftKey && !e.altKey && (e.key === "k" || e.key === "f")) {
      e.preventDefault();
      searchRef?.focus();
      searchRef?.select();
    } else if (mod && !e.shiftKey && !e.altKey && e.key === "n") {
      e.preventDefault();
      setShowNew(true);
    } else if (
      e.altKey &&
      !e.shiftKey &&
      (e.key === "ArrowUp" || e.key === "ArrowDown") &&
      (mod || !(e.target instanceof HTMLElement && e.target.closest("input, textarea")))
    ) {
      const list = visibleConversations();
      if (list.length === 0) return;
      e.preventDefault();
      const i = list.findIndex((c) => c.id === selectedConversationId());
      const next =
        i < 0 ? 0 : e.key === "ArrowUp" ? Math.max(0, i - 1) : Math.min(list.length - 1, i + 1);
      open(list[next].id, true);
      document
        .querySelector(`.conversation-list > :nth-child(${next + 1})`)
        ?.scrollIntoView({ block: "nearest" });
    }
  }
  onMount(() => window.addEventListener("keydown", onGlobalKey));
  onCleanup(() => window.removeEventListener("keydown", onGlobalKey));

  return (
    <div class="chats-split">
      <div class="chats-list-panel">
        {/* The header row is the window drag strip (Signal/WhatsApp): the title
            rides up alongside the macOS traffic lights. The new-message button is
            a child without the attribute, so it stays clickable. */}
        <div class="chats-header" data-tauri-drag-region>
          <span class="chats-header-title" data-tauri-drag-region>
            Chats
            {totalUnread() > 0 && (
              <span class="chats-unread-badge">{totalUnread()}</span>
            )}
          </span>
          <button
            class="chats-new-btn"
            onClick={() => setShowNew(true)}
            aria-label="New message"
            title={isMac ? "New message (⌘N)" : "New message (Ctrl+N)"}
          >
            <FiEdit size={18} />
          </button>
        </div>
        <Show when={showsAccountTabs()}>
          <div class="account-tabs scrollbar-thin" role="tablist" aria-label="Accounts">
            <For each={store.accounts}>
              {(account) => (
                <button
                  class="account-tab"
                  classList={{ selected: selectedAccountTab() === account.id }}
                  role="tab"
                  aria-selected={selectedAccountTab() === account.id}
                  title={accountLabel(account)}
                  onClick={() => setSelectedChatsAccountTab(account.id)}
                >
                  <span class="account-tab-icon">
                    <AccountAvatar name={accountLabel(account)} did={account.id} />
                    <Show when={unreadFor(account.id) > 0}>
                      <span class="account-tab-badge">{unreadFor(account.id)}</span>
                    </Show>
                  </span>
                  <span class="account-tab-label">{accountLabel(account)}</span>
                </button>
              )}
            </For>
          </div>
        </Show>
        <div class="chats-search">
          <FiSearch size={14} class="chats-search-icon" aria-hidden="true" />
          <input
            ref={searchRef}
            class="chats-search-input"
            type="text"
            placeholder="Search"
            aria-label="Search conversations"
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={onSearchKey}
            spellcheck={false}
          />
          <Show
            when={query()}
            fallback={<kbd class="chats-search-hint">{isMac ? "⌘K" : "Ctrl K"}</kbd>}
          >
            <button class="chats-search-clear" onClick={clearSearch} aria-label="Clear search">
              <FiX size={13} />
            </button>
          </Show>
        </div>
        <RecoveryKeyBanner />
        <OfflineBanner />
        <div class="conversation-list scrollbar-thin">
          <For
            each={visibleConversations()}
            fallback={
              <div class="empty-state">
                {query().trim()
                  ? `No conversations match "${query().trim()}".`
                  : "No conversations yet. Join a server to get started."}
              </div>
            }
          >
            {(conv) => (
              <ConversationRow
                conversation={conv}
                selected={selectedConversationId() === conv.id}
                onSelect={(id) => open(id, false)}
              />
            )}
          </For>
        </div>
      </div>
      <div class="detail-panel">
        <Show
          when={selected()}
          fallback={
            <div class="no-selection" data-tauri-drag-region>
              Select a conversation
            </div>
          }
        >
          {(conv) => <ConversationView conversation={conv()} />}
        </Show>
      </div>
      <Show when={showNew()}>
        <NewConversationView onClose={() => setShowNew(false)} />
      </Show>
    </div>
  );
}
