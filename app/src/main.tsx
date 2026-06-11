import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

export type ThemePref = "light" | "dark" | "system";

export function applyTheme(pref: ThemePref) {
  const dark =
    pref === "dark" ||
    (pref === "system" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

applyTheme((localStorage.getItem("zn-theme") as ThemePref) ?? "system");
window
  .matchMedia("(prefers-color-scheme: dark)")
  .addEventListener("change", () => {
    const pref = (localStorage.getItem("zn-theme") as ThemePref) ?? "system";
    if (pref === "system") applyTheme(pref);
  });

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
