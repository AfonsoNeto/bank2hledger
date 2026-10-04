import React from "react";
import ReactDOM from "react-dom/client";
import { FluentProvider, webDarkTheme, webLightTheme } from "@fluentui/react-components";
import { App } from "./App";
import "./styles.css";

function usePrefersDark(): boolean {
  const [dark, setDark] = React.useState(
    () => window.matchMedia("(prefers-color-scheme: dark)").matches,
  );
  React.useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = (e: MediaQueryListEvent) => setDark(e.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);
  return dark;
}

// A plain browser has no Mica behind the transparent window — flag it so
// styles.css paints a themed backdrop instead (the Tauri window stays
// transparent and keeps letting Mica show through).
if (typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window)) {
  document.documentElement.classList.add("browser-mode");
}

function ThemedApp() {
  const dark = usePrefersDark();
  return (
    <FluentProvider theme={dark ? webDarkTheme : webLightTheme} style={{ background: "transparent", height: "100%" }}>
      <App />
    </FluentProvider>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ThemedApp />
  </React.StrictMode>,
);
