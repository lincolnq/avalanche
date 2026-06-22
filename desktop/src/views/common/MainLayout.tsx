import { useLocation, A } from "@solidjs/router";
import type { RouteSectionProps } from "@solidjs/router";
import type { JSX } from "solid-js";
import { useApp } from "../../state/AppContext";

const NAV_ITEMS: Array<{ path: string; label: string; icon: string }> = [
  { path: "/chats", label: "Chats", icon: "💬" },
  { path: "/network", label: "Network", icon: "🌐" },
];

const styles = `
  .layout {
    display: flex;
    height: 100vh;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    background: #FFF1E9;
    color: #1F1815;
  }
  .sidebar {
    width: 64px;
    background: #2A1620;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 16px 0;
    gap: 8px;
    flex-shrink: 0;
  }
  .sidebar-link {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    width: 48px;
    height: 48px;
    border-radius: 12px;
    text-decoration: none;
    color: rgba(255, 255, 255, 0.55);
    font-size: 22px;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .sidebar-link:hover {
    background: rgba(255, 255, 255, 0.08);
    color: rgba(255, 255, 255, 0.85);
  }
  .sidebar-link.active {
    background: rgba(255, 255, 255, 0.15);
    color: #fff;
  }
  .sidebar-label {
    font-size: 9px;
    margin-top: 2px;
    letter-spacing: 0.03em;
  }
  .sidebar-spacer {
    flex: 1;
  }
  .logout-btn {
    width: 48px;
    height: 48px;
    border-radius: 12px;
    background: transparent;
    border: none;
    color: rgba(255, 255, 255, 0.45);
    cursor: pointer;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    font-size: 18px;
    gap: 2px;
    transition: background 0.15s, color 0.15s;
  }
  .logout-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: rgba(255, 255, 255, 0.75);
  }
  .logout-label {
    font-size: 9px;
    letter-spacing: 0.03em;
  }
  .content {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
`;

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
    <>
      <style>{styles}</style>
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
    </>
  );
}
