import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/recursive/full.css";
import "../tokens.css";
import "./chip.css";
import { Chip } from "./Chip";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Chip />
  </React.StrictMode>,
);
