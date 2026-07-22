import { defineConfig } from "vite";
import tailwindcss from "@tailwindcss/vite";
import { viteSingleFile } from "vite-plugin-singlefile";

// The editor is embedded into the plugin dynamic library as a single, fully
// self-contained `index.html` (JS/CSS/assets inlined). It is served to CEF via
// the `mikoplugin://<plugin>/index.html` custom scheme, so there are no sibling
// asset requests to resolve.
export default defineConfig({
  plugins: [tailwindcss(), viteSingleFile()],
  // This package lives inside a Bun workspace, so a dependency can resolve
  // `react` to the hoisted root copy while the app resolves its own — two React
  // module instances end up inlined in the same bundle. The second copy has a
  // null hook dispatcher, so the first component from a library that imported it
  // (react-router-dom's `HashRouter`) throws
  // "Cannot read properties of null (reading 'useRef')" at mount, the editor
  // never reaches `sendBridgeReady`, and the host's watchdog reloads it until it
  // gives up. Deduping pins every importer to one copy.
  resolve: {
    dedupe: ["react", "react-dom", "react-router-dom"],
  },
  build: {
    assetsInlineLimit: Infinity,
    cssCodeSplit: false,
  },
});
