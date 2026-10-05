import { useLocation, useNavigate, A } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import type { JSX } from "solid-js";
import { createEffect, on, Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import { TbOutlineSettings, TbOutlineRefresh } from "solid-icons/tb";
import { ChatsBubbleIcon, NetworkGlobeIcon } from "../../components/icons/BrandIcons";
import { useApp } from "../../state/AppContext";
import { updateStatus, installAndRestart } from "../../state/updater";
import "./MainLayout.css";

type NavItem = { path: string; label: string; icon: typeof ChatsBubbleIcon };

const NAV_ITEMS: NavItem[] = [
  // Same glyphs as the iOS tab bar (MainTabView: TabChats / TabNetwork).
  { path: "/chats", label: "Chats", icon: ChatsBubbleIcon },
  { path: "/network", label: "Network", icon: NetworkGlobeIcon },
];

interface NavLinkProps {
  item: NavItem;
}

function NavLink(props: NavLinkProps) {
  const location = useLocation();
  const isActive = () =>
    location.pathname === props.item.path ||
    (props.item.path === "/chats" &&
      (location.pathname === "/" ||
        location.pathname.startsWith("/chats")));

  return (
    <A
      href={props.item.path}
      class={`sidebar-link${isActive() ? " active" : ""}`}
      aria-label={props.item.label}
    >
      <Dynamic component={props.item.icon} size={24} />
      <span class="sidebar-label">{props.item.label}</span>
    </A>
  );
}

export default function MainLayout(props: RouteSectionProps): JSX.Element {
  const { selectedConversationId } = useApp();
  const navigate = useNavigate();
  const location = useLocation();

  // When a conversation is selected programmatically while the user isn't on
  // the Chats route — a deep link (in-webview project link or an external
  // avalanche:///go.theavalanche.net link) or a newly created DM — switch to
  // Chats so the selection is visible. ChatsView renders the selection from the
  // signal; from within Chats this is a no-op. `defer` skips the mount run.
  const onChats = () =>
    location.pathname === "/" || location.pathname.startsWith("/chats");
  createEffect(
    on(
      selectedConversationId,
      (id) => {
        if (id && !onChats()) navigate("/chats");
      },
      { defer: true }
    )
  );

  return (
    <div class="layout">
      {/* The sidebar's empty areas (top inset under the macOS traffic lights and
          the flex spacer) drag the window; the icon links/buttons are children
          without the attribute, so they stay clickable. */}
      <nav class="sidebar" data-tauri-drag-region>
        {NAV_ITEMS.map((item) => (
          <NavLink item={item} />
        ))}
        <div class="sidebar-spacer" data-tauri-drag-region />
        {/* A downloaded update waits for the user (docs/63): never auto-restart. */}
        <Show when={updateStatus().kind === "ready" || updateStatus().kind === "installing"}>
          <button
            class="sidebar-update"
            onClick={() => void installAndRestart()}
            disabled={updateStatus().kind === "installing"}
            title="Restart to install the update"
          >
            <TbOutlineRefresh size={18} aria-hidden="true" />
            <span class="sidebar-update-label">
              {updateStatus().kind === "installing" ? "Updating…" : "Restart to update"}
            </span>
          </button>
        </Show>
        <A href="/settings" class="sidebar-settings-link" aria-label="Settings" title="Settings">
          {/* Outlined gear standing in for iOS's SF Symbol `gearshape` (SF
              Symbols are licensed for Apple platforms only). currentColor, so
              it takes the link color + hover. */}
          <TbOutlineSettings size={24} aria-hidden="true" />
        </A>
        {/* No Sign out here: it forgets every signed-in account, so it lives in
            Settings > Developer (iOS has no user-facing sign-out at all). */}
      </nav>
      <main class="content">
        {props.children}
      </main>
    </div>
  );
}
