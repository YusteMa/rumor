import es from "./i18n/es.json";
import en from "./i18n/en.json";

// Phase 0 placeholder: proves the WebView, the bundler and i18n are wired up.
// The real panel (Phase 3) only talks to Rust through `invoke` and events.
const strings = navigator.language.toLowerCase().startsWith("es") ? es : en;

const status = document.getElementById("status");
if (status) status.textContent = strings.skeletonReady;
