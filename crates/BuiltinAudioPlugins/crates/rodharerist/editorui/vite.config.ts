import { defineConfig } from "vite";
import tailwindcss from "@tailwindcss/vite";
import { viteSingleFile } from "vite-plugin-singlefile";

// The editor is embedded into the plugin dynamic library as a single, fully
// self-contained `index.html` (JS/CSS/assets inlined). It is served to CEF via
// the `mikoplugin://<plugin>/index.html` custom scheme, so there are no sibling
// asset requests to resolve.
export default defineConfig({
  plugins: [tailwindcss(), viteSingleFile()],
  build: {
    assetsInlineLimit: Infinity,
    cssCodeSplit: false,
  },
});
