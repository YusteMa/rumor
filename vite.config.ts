import { resolve } from "node:path";
import { defineConfig } from "vite";

// The panel lives in src/ (see CLAUDE.md), so that is the Vite root.
export default defineConfig({
  root: "src",
  // Tauri expects a fixed port and its own Rust errors must stay visible.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    // WebView2 is evergreen Chromium.
    target: "chrome120",
    // Four pages: the tray panel, Settings, Statistics and the mini clock.
    rollupOptions: {
      input: {
        panel: resolve(import.meta.dirname, "src/index.html"),
        settings: resolve(import.meta.dirname, "src/settings.html"),
        stats: resolve(import.meta.dirname, "src/stats.html"),
        mini: resolve(import.meta.dirname, "src/mini.html"),
      },
    },
  },
});
