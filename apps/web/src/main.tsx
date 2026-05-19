import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import "./styles/index.css";
import App from "./App.tsx";
import { ExternalMixerWindow } from "./routes/ExternalMixerWindow";
// Supports weights 100-900
import "@fontsource-variable/inter/opsz.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <HashRouter>
      <Routes>
        <Route path="/external/mixer" element={<ExternalMixerWindow />} />
        <Route path="/" element={<App />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </HashRouter>
  </StrictMode>,
);
