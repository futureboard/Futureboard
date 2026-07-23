import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { fileURLToPath } from 'node:url'

export default defineConfig({
  // The checkout may be reached through H:\ProjectsDev or its canonical
  // W:\works junction. Pinning root to this config's canonical URL prevents
  // Vite 8/Rolldown from treating index.html as an absolute emitted asset.
  root: fileURLToPath(new URL('.', import.meta.url)),
  plugins: [react()],
  build: {
    assetsInlineLimit: Infinity,
    cssCodeSplit: false,
  },
})
