import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/recursive/full.css";
import "./styles.css";
import App from "./App";
import { isWindows } from "./platform";

// styles.css has a short Windows section: no traffic lights, no vibrancy, Segoe UI.
if (isWindows) document.documentElement.classList.add("os-windows");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
