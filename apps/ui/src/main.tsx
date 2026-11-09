import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { TooltipProvider } from "@/components/dsgn/tooltip";
import "./styles/base.css";
import { App } from "./App";

// Theme before first paint, so there is no flash of the wrong palette.
try {
  const t = localStorage.getItem("selecta.theme");
  if (t === "light" || t === "dark") document.documentElement.dataset.theme = t;
} catch {
  /* storage unavailable: default dark */
}

if (import.meta.env.DEV) {
  // Console/CDP handle for measuring the audio chain: `await selectaSelfTest()`.
  const { selfTest } = await import("@selecta/engine");
  Object.assign(window, { selectaSelfTest: selfTest });
}

if (import.meta.env.DEV && new URLSearchParams(location.search).has("mock")) {
  const { installMockBackend } = await import("./dev/mock-backend");
  await installMockBackend();
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <TooltipProvider delayDuration={400}>
      <App />
    </TooltipProvider>
  </StrictMode>,
);
