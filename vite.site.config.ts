import { execFileSync } from "node:child_process";
import { cpSync, existsSync } from "node:fs";
import { resolve } from "node:path";
import { defineConfig, type Plugin } from "vite";

// La web de Rumor (SPEC §10.2), en web/site/. Se compila con
// `npm run build:site`, que antes compila la demo del panel (`build:web`).
// Sale en web/site-dist/, lista para subir a Cloudflare Pages.

/**
 * La última versión publicada, leída del `latest.json` de la rama `updater`
 * (el mismo que usa la auto-actualización de la app, SPEC §7). Con ella el
 * botón de Windows descarga el instalador directamente. Si no se puede leer
 * (sin conexión, sin la rama), la web enlaza a la página de versiones.
 */
function latestRelease(): { version: string; url: string } | null {
  try {
    execFileSync("git", ["fetch", "--quiet", "origin", "updater"], { stdio: "ignore" });
    const json = execFileSync("git", ["show", "origin/updater:latest.json"], { encoding: "utf8" });
    const version: unknown = JSON.parse(json).version;
    if (typeof version !== "string" || !/^[\w.-]+$/.test(version)) return null;
    const url = `https://github.com/YusteMa/rumor/releases/download/v${version}/Rumor_${version}_x64-setup.exe`;
    return { version, url };
  } catch {
    return null;
  }
}

/** Copia la demo del panel (web/dist, de `npm run build:web`) dentro de la web, en demo/. */
function copyDemo(): Plugin {
  return {
    name: "rumor-copy-demo",
    apply: "build",
    closeBundle() {
      const demo = resolve(import.meta.dirname, "web/dist");
      if (!existsSync(demo)) throw new Error("Falta web/dist: ejecuta antes `npm run build:web`.");
      cpSync(demo, resolve(import.meta.dirname, "web/site-dist/demo"), { recursive: true });
    },
  };
}

export default defineConfig({
  root: "web/site",
  base: "/",
  define: {
    __RUMOR_RELEASE__: JSON.stringify(latestRelease()),
  },
  plugins: [copyDemo()],
  build: {
    outDir: "../site-dist",
    emptyOutDir: true,
    target: "es2022",
  },
});
