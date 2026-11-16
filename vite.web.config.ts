import { resolve } from "node:path";
import { defineConfig } from "vite";

// The panel and Statistics, for the demo on marioyuste.com (web/README.md). Mode "web"
// makes api.ts play through the engine compiled to WebAssembly.
export default defineConfig({
  root: "src",
  mode: "web",
  // Relative paths, so it works from any folder of the website.
  base: "./",
  build: {
    outDir: "../web/dist",
    emptyOutDir: true,
    target: "es2022",
    rollupOptions: {
      input: {
        panel: resolve(import.meta.dirname, "src/index.html"),
        // Statistics with sample data, opened from the panel's button.
        stats: resolve(import.meta.dirname, "src/stats.html"),
      },
    },
  },
});
