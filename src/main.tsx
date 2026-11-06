/// <reference types="vite/client" />
import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/recursive/full.css";
import notepadFont from "@fontsource-variable/recursive/files/recursive-latin-full-normal.woff2?url";
import "./styles.css";
import App from "./App";

// Fetch the notepad font now. Otherwise it starts only when the first text that uses it is
// laid out, after `boot` returns, and the first screen briefly shows the fallback font.
document.head.append(Object.assign(document.createElement("link"), { rel: "preload", as: "font", type: "font/woff2", href: notepadFont, crossOrigin: "anonymous" }));

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
