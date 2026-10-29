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
  },
});
