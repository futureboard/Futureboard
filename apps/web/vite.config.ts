import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

import tailwindcss from "@tailwindcss/vite";

// https://vite.dev/config/
export default defineConfig({
  // Use relative asset paths so the built bundle can be loaded from
  // a `file://` URL inside Electron when packaged.
  base: "./",
  plugins: [react(), tailwindcss()],
});
