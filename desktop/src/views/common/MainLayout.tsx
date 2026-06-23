import { useLocation, A } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import type { JSX } from "solid-js";
import { useApp } from "../../state/AppContext";
import "./MainLayout.css";

const NAV_ITEMS: Array<{ path: string; label: string; icon: string }> = [
  { path: "/chats", label: "Chats", icon: "💬" },
  { path: "/network", label: "Network", icon: "🌐" },
];

interface NavLinkProps {
  item: { path: string; label: string; icon: string };
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
      <span>{props.item.icon}</span>
      <span class="sidebar-label">{props.item.label}</span>
    </A>
  );
}

export default function MainLayout(props: RouteSectionProps): JSX.Element {
  const { logout } = useApp();

  return (
    <div class="layout">
      <nav class="sidebar">
        {NAV_ITEMS.map((item) => (
          <NavLink item={item} />
        ))}
        <div class="sidebar-spacer" />
        <button class="logout-btn" onClick={logout} title="Sign out">
          <span>↩</span>
          <span class="logout-label">Sign out</span>
        </button>
      </nav>
      <main class="content">
        {props.children}
      </main>
    </div>
  );
}
