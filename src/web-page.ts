// What every Rumor page needs when it is shown on a web page (the web build,
// `npm run build:web`) instead of in a Tauri window: a theme the host page
// controls, and a clear label when the data is made up.

export interface WebPageOptions {
  /** The page shows the browser stand-in's sample data. */
  sample?: boolean;
}

export function webPage({ sample = false }: WebPageOptions = {}): void {
  const root = document.documentElement;
  const params = new URLSearchParams(location.search);
  root.dataset.web = "";
  // Dark unless the host asks otherwise (`?theme=light`, or later a message).
  root.dataset.theme = params.get("theme") === "light" ? "light" : "dark";
  addEventListener("message", (e) => {
    const theme = e.data?.rumorTheme;
    if (e.origin === location.origin && (theme === "light" || theme === "dark")) {
      setTheme(theme);
    }
  });

  // Some colours transition (the accent, 300 ms; some surfaces, 250 ms) and
  // others snap, so a live theme change flickered through mixed states. Now
  // every colour switches at once, with transitions off, and the browser
  // crossfades the whole page from the old picture to the new one.
  function setTheme(theme: "light" | "dark") {
    if (root.dataset.theme === theme) return;
    const apply = () => {
      root.classList.add("web-theme-switch");
      root.dataset.theme = theme;
      void root.offsetWidth; // apply the new colours now, without transitions
      requestAnimationFrame(() => root.classList.remove("web-theme-switch"));
    };
    const calm = matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (document.startViewTransition && !calm) document.startViewTransition(apply);
    else apply();
  }

  const style = document.createElement("style");
  style.textContent = `
    .web-theme-switch,
    .web-theme-switch *,
    .web-theme-switch *::before,
    .web-theme-switch *::after { transition: none !important; }
    ::view-transition-old(root),
    ::view-transition-new(root) { animation-duration: 320ms; animation-timing-function: ease-in-out; }
    /* The footer (Settings, shortcut, Quit) only makes sense in Windows. */
    :root[data-web] .foot { display: none; }
    .web-sample {
      margin: 0;
      padding: var(--space-2) var(--space-3);
      background: var(--accent-soft);
      color: var(--ink);
      font: var(--weight-semibold) var(--text-xs) / 1.4 var(--font);
      letter-spacing: 0.06em;
      text-align: center;
    }
    .web-sample span { font-weight: var(--weight-regular); letter-spacing: 0; color: var(--muted); }
  `;
  document.head.append(style);

  if (sample) {
    const es = (params.get("lang") ?? navigator.language).startsWith("es");
    const note = document.createElement("p");
    note.className = "web-sample";
    note.setAttribute("role", "note");
    note.innerHTML = es
      ? "DATOS DE EJEMPLO <span>· Diez semanas inventadas para enseñar la ventana. No son de nadie.</span>"
      : "SAMPLE DATA <span>· Ten made-up weeks to show the window. They belong to no one.</span>";
    document.body.prepend(note);
  }
}
