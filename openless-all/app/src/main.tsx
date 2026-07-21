import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { detectOS } from "./components/WindowChrome";
import i18n from "./i18n"; // 副作用：触发 i18next init
import { initThemeMode } from "./lib/themeMode";
import "./styles/tokens.css";
import "./styles/global.css";

import type { OS } from "./components/WindowChrome";

const params = new URLSearchParams(window.location.search);
const windowKind = params.get("window");
const isCapsule = windowKind === "capsule";
const isQa = windowKind === "qa";
const isSelectionPolishPreview = windowKind === "selection-polish-preview";
const isLessComputer = windowKind === "less-computer";
const isLessComputerGlow = windowKind === "less-computer-glow";
const osQuery = params.get("os") as OS | null;
const os = osQuery ?? detectOS();
document.documentElement.dataset.olPlatform = os;
initThemeMode();

const root = ReactDOM.createRoot(document.getElementById("root")!);

const renderApp = () => {
  root.render(
    <React.StrictMode>
      <App
        isCapsule={isCapsule}
        isQa={isQa}
        isSelectionPolishPreview={isSelectionPolishPreview}
        isLessComputer={isLessComputer}
        isLessComputerGlow={isLessComputerGlow}
        forcedOs={os}
      />
    </React.StrictMode>,
  );
};

// i18n 必须就绪后才能渲染：否则首次渲染拿到的 t() 返回 key 字面量。
// react-i18next useSuspense=false 时不会自动等，只有事件触发后重渲染才能拿到译文。
if (i18n.isInitialized) {
  renderApp();
} else {
  i18n.on("initialized", renderApp);
}
