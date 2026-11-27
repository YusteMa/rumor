// La web de Rumor (SPEC §10.2): idioma, dispositivo del visitante, la demo
// del panel y sus estadísticas. Sin frameworks (regla 7): solo el DOM.

import "../../src/styles/tokens.css";
import "./site.css";
import { en, es, type Key, type Lang } from "./i18n";
// Vite convierte cada `import x from "....png"` en la dirección final de la
// imagen (con un hash en el nombre, para que el navegador la guarde en caché).
import panelNoise from "../../docs/screenshots/panel-noise.png";
import panelStudy from "../../docs/screenshots/panel-study.png";
import statistics from "../../docs/screenshots/statistics.png";

/**
 * La última versión de Windows, leída al compilar (vite.site.config.ts).
 * `declare const` es como un `extern` en C++: la variable existe, pero la
 * define otro (aquí, Vite, que pone el valor al compilar).
 */
declare const __RUMOR_RELEASE__: { version: string; url: string } | null;

const RELEASES_URL = "https://github.com/YusteMa/rumor/releases";
const LANG_KEY = "rumor-site-lang";
const root = document.documentElement;

// ---------- Idioma ----------

/** El idioma: el que se eligió antes, si no `?lang=`, si no el del navegador. */
function pickLang(): Lang {
  const asked = new URLSearchParams(location.search).get("lang");
  if (asked === "es" || asked === "en") return asked;
  try {
    const saved = localStorage.getItem(LANG_KEY);
    if (saved === "es" || saved === "en") return saved;
  } catch {
    // Sin almacenamiento (modo privado): se usa el del navegador.
  }
  return navigator.language.toLowerCase().startsWith("es") ? "es" : "en";
}

let lang: Lang = pickLang();
const t = (key: Key) => (lang === "es" ? es : en)[key];

/** Pone todos los textos de la página en el idioma actual. */
function applyLang() {
  root.lang = lang;
  document.querySelectorAll<HTMLElement>("[data-i18n]").forEach((el) => {
    el.textContent = t(el.dataset.i18n as Key);
  });
  document.querySelectorAll<HTMLElement>("[data-i18n-aria]").forEach((el) => {
    el.setAttribute("aria-label", t(el.dataset.i18nAria as Key));
  });
  const toggle = document.querySelector<HTMLButtonElement>("[data-lang-toggle]");
  if (toggle) {
    toggle.textContent = lang === "es" ? "EN" : "ES";
    toggle.setAttribute("aria-label", t("langToggle"));
  }
  shots.forEach(([img, key]) => (img.alt = t(key)));
  panel.title = t("panelTitle");
  statsFrame.title = t("statsFrame");
  showDevice();
  showRelease();
}

document.querySelector("[data-lang-toggle]")?.addEventListener("click", () => {
  lang = lang === "es" ? "en" : "es";
  try {
    localStorage.setItem(LANG_KEY, lang);
  } catch {
    // Se queda solo para esta visita.
  }
  applyLang();
  // El panel también cambia de idioma; para eso hay que volver a cargarlo
  // (si estaba sonando, se para: es un cambio que hace la persona).
  if (panel.src) loadPanel();
});

// ---------- Dispositivo del visitante ----------

type Platform = "windows" | "ios" | "android" | "desktop";

/**
 * Adivina el sistema por el «user agent», el texto con el que se presenta
 * el navegador. El iPad moderno dice ser un Mac, así que se le reconoce por
 * la pantalla táctil (`maxTouchPoints`).
 */
function detectPlatform(): Platform {
  const ua = navigator.userAgent;
  if (/iPhone|iPad|iPod/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1)) return "ios";
  if (/Android/.test(ua)) return "android";
  if (/Windows/.test(ua)) return "windows";
  return "desktop";
}

const platform = detectPlatform();

/** Marca la tarjeta del dispositivo del visitante y elige el botón principal de la portada. */
function showDevice() {
  document.querySelectorAll<HTMLElement>("[data-platform]").forEach((card) => {
    const mine = card.dataset.platform === platform;
    card.classList.toggle("platform--yours", mine);
    card.querySelector<HTMLElement>(".badge--yours")?.toggleAttribute("hidden", !mine);
  });

  const primary = document.querySelector<HTMLAnchorElement>("[data-primary]");
  const secondary = document.querySelector<HTMLAnchorElement>("[data-secondary]");
  if (!primary || !secondary) return;
  if (platform === "windows") {
    // En Windows, el botón grande descarga directamente; el otro lleva a la demo.
    primary.textContent = t("ctaDownloadWindows");
    primary.href = windowsUrl();
    secondary.textContent = t("ctaListen");
    secondary.href = "#escuchar";
  } else {
    // En el resto, de momento lo que se puede hacer es escucharlo aquí.
    primary.textContent = t("ctaListen");
    primary.href = "#escuchar";
    secondary.textContent = t("ctaWhere");
    secondary.href = "#descargar";
  }
}

// ---------- Descarga de Windows ----------

/** El instalador de la última versión o, si no se supo al compilar, la página de versiones. */
const windowsUrl = () => __RUMOR_RELEASE__?.url ?? RELEASES_URL;

function showRelease() {
  const link = document.querySelector<HTMLAnchorElement>("[data-windows-download]");
  if (link) link.href = windowsUrl();
  const meta = document.querySelector<HTMLElement>("[data-windows-meta]");
  if (meta && __RUMOR_RELEASE__) meta.textContent = t("windowsMeta").replace("{version}", __RUMOR_RELEASE__.version);
}

// ---------- Capturas ----------

// Pares [imagen, clave de su texto alternativo]. `as const` deja el par como
// una tupla fija, como un std::pair.
const shots = (
  [
    ["panelNoise", panelNoise, "shotNoise"],
    ["panelStudy", panelStudy, "shotStudy"],
    ["statistics", statistics, "shotStats"],
  ] as const
).flatMap(([name, url, key]) => {
  const img = document.querySelector<HTMLImageElement>(`[data-shot="${name}"]`);
  if (!img) return [];
  img.src = url;
  return [[img, key] as const];
});

// ---------- La demo del panel ----------

const panel = document.querySelector<HTMLIFrameElement>("[data-panel]") as HTMLIFrameElement;
const statsDialog = document.querySelector<HTMLDialogElement>("[data-stats]") as HTMLDialogElement;
const statsFrame = document.querySelector<HTMLIFrameElement>("[data-stats-frame]") as HTMLIFrameElement;
const dark = matchMedia("(prefers-color-scheme: dark)");
const theme = () => (dark.matches ? "dark" : "light");

/** Carga el panel (compilado con `npm run build:web`, en demo/) con el idioma y el tema de la página. */
function loadPanel() {
  // "demo/" y no "demo/index.html": Cloudflare Pages redirige lo segundo a lo
  // primero, y así el panel se ahorra un viaje.
  panel.src = `demo/?study=1&lang=${lang}&theme=${theme()}`;
}

// El panel se carga cuando la demo está a punto de verse, no antes: así la
// portada carga rápido y el motor de audio solo se descarga si hace falta.
new IntersectionObserver(
  (entries, observer) => {
    if (entries.some((e) => e.isIntersecting)) {
      loadPanel();
      observer.disconnect();
    }
  },
  { rootMargin: "400px" },
).observe(panel);

// Si el sistema cambia entre claro y oscuro, el panel cambia con la página.
dark.addEventListener("change", () => {
  panel.contentWindow?.postMessage({ rumorTheme: theme() }, location.origin);
});

// El panel cuenta lo que pasa con mensajes. Solo se aceptan si vienen de
// esta misma web (`origin`) y de uno de nuestros iframes.
addEventListener("message", (e) => {
  if (e.origin !== location.origin) return;
  if (e.source !== panel.contentWindow && e.source !== statsFrame.contentWindow) return;

  // El color de la página sigue al ruido que suena, como en la app.
  const color = e.data?.rumorState?.color;
  if (color === "brown" || color === "white") root.dataset.noise = color;
  else if (color === "pink") delete root.dataset.noise;

  // El botón «Estadísticas» del panel pide abrirlas.
  if (e.data?.rumorOpen === "stats") {
    const c = typeof e.data.color === "string" ? e.data.color : "pink";
    statsFrame.src = `demo/stats.html?color=${encodeURIComponent(c)}&lang=${lang}&theme=${theme()}`;
    statsDialog.showModal();
  }
});

// Al cerrar las estadísticas se vacía el iframe, para que no siga gastando.
statsDialog.addEventListener("close", () => statsFrame.removeAttribute("src"));
// Un clic fuera de la ventana (en el fondo oscurecido) también la cierra.
statsDialog.addEventListener("click", (e) => {
  if (e.target === statsDialog) statsDialog.close();
});

// ---------- Entradas suaves al hacer scroll ----------

// Cada bloque aparece con un pequeño fundido cuando entra en pantalla. Con
// «reducir movimiento» activado, site.css lo deja quieto.
const reveal = new IntersectionObserver(
  (entries) => {
    for (const e of entries) {
      if (e.isIntersecting) {
        e.target.classList.add("is-in");
        reveal.unobserve(e.target);
      }
    }
  },
  { rootMargin: "0px 0px -10% 0px" },
);
document.querySelectorAll(".section__head, .card, .feature, .platform, .demo__body").forEach((el) => {
  el.classList.add("reveal");
  reveal.observe(el);
});

applyLang();
