import { contextBridge } from "electron";

const isMac = process.platform === "darwin";
/** `titleBarOverlay` in main enables Chromium Window Controls Overlay (Windows / Linux). */
const windowControlsOverlayEnabled = !isMac;

contextBridge.exposeInMainWorld("dawElectron", {
  platform: process.platform,
  frameless: true,
  transparentWindow: true,
  windowControlsOverlayEnabled,
});
