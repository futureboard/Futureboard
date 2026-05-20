import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import "./styles/index.css";
import App from "./App.tsx";
import { ExternalMixerWindow } from "./routes/ExternalMixerWindow";
import { ExternalProjectWizardWindow } from "./routes/ExternalProjectWizardWindow";
import { ExternalSettingsWindow } from "./routes/ExternalSettingsWindow";
import { ExternalPluginManagerWindow } from "./routes/ExternalPluginManagerWindow";
// Supports weights 100-900
import "@fontsource-variable/inter/opsz.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <HashRouter>
      <Routes>
        <Route path="/external/mixer" element={<ExternalMixerWindow />} />
        <Route path="/projectwizard" element={<ExternalProjectWizardWindow />} />
        <Route path="/settings" element={<ExternalSettingsWindow />} />
        <Route path="/plugin-manager" element={<ExternalPluginManagerWindow />} />
        <Route path="/" element={<App />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </HashRouter>
  </StrictMode>,
);
