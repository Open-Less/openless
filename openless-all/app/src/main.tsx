import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import "./styles/tokens.css";
import "./styles/global.css";

const params = new URLSearchParams(window.location.search);
const isCapsule = params.get("window") === "capsule";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App isCapsule={isCapsule} />
  </React.StrictMode>,
);
