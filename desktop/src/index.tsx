Object.freeze(Object.prototype);

// The main window hands file drops to the page (dragDropEnabled: false) so the
// composer can attach dropped images. A drop anywhere else must not fall
// through to the webview default, which would navigate to the file and
// replace the app.
window.addEventListener("dragover", (e) => e.preventDefault());
window.addEventListener("drop", (e) => e.preventDefault());

// macOS uses the `titleBarStyle: "Overlay"` window (no native title bar; traffic
// lights overlay the top-left). Tag the root so the traffic-light inset in the
// sidebar is applied only there — Windows/Linux keep their native title bar and
// need no inset. userAgent is available without any Tauri plugin/capability.
if (typeof navigator !== "undefined" && /Mac/i.test(navigator.userAgent)) {
  document.documentElement.classList.add("is-macos");
}

import { render } from "solid-js/web";
import { AppProvider } from "./state/AppContext";
import App from "./App";
import "./styles/theme.css";

const root = document.getElementById("root");
if (!root) throw new Error("No root element");

render(
  () => (
    <AppProvider>
      <App />
    </AppProvider>
  ),
  root
);
