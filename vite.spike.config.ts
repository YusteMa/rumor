import { defineConfig } from "vite";

// Fase W0 (ROADMAP): la página de prueba del audio en segundo plano en el
// móvil, en src/spike/. Se borra cuando la W0 tenga su respuesta.
export default defineConfig({
  root: "src/spike",
  base: "./",
  build: {
    outDir: "../../web/dist-spike",
    emptyOutDir: true,
    target: "es2022",
  },
});
