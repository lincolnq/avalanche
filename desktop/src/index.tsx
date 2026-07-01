Object.freeze(Object.prototype);

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
