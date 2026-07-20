import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { getStoredTheme } from "./lib/api";

// Apply the saved theme BEFORE first render so there's no flash of the wrong
// colors. data-theme drives the CSS variable blocks in App.css.
document.documentElement.dataset.theme = getStoredTheme();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
