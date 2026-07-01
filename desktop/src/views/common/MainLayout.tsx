import { useLocation, A } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import type { JSX } from "solid-js";
import { Dynamic } from "solid-js/web";
import { FiSettings, FiMessageSquare, FiGlobe, FiLogOut } from "solid-icons/fi";
import { useApp } from "../../state/AppContext";
import "./MainLayout.css";

type NavItem = { path: string; label: string; icon: typeof FiMessageSquare };

const NAV_ITEMS: NavItem[] = [
  { path: "/chats", label: "Chats", icon: FiMessageSquare },
  { path: "/network", label: "Network", icon: FiGlobe },
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
      <Dynamic component={props.item.icon} size={22} />
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
        <A href="/settings" class="sidebar-settings-link" aria-label="Settings" title="Settings">
          {/* Feather "settings" outlined gear, matching iOS SF Symbol
              `gearshape` and Android's Material settings icon. Renders with
              stroke="currentColor", so it inherits the link color + hover. */}
          <FiSettings size={22} aria-hidden="true" />
        </A>
        <button class="logout-btn" onClick={logout} title="Sign out">
          <FiLogOut size={20} aria-hidden="true" />
          <span class="logout-label">Sign out</span>
        </button>
      </nav>
      <main class="content">
        {props.children}
      </main>
    </div>
  );
}
