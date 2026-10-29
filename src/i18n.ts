import es from "./i18n/es.json";
import en from "./i18n/en.json";
import type { Lang } from "./api";

type Key = keyof typeof en;
const tables: Record<Lang, Record<Key, string>> = { es, en };

let lang: Lang = navigator.language.toLowerCase().startsWith("es") ? "es" : "en";

export function setLang(next: Lang): void {
  lang = next;
  document.documentElement.lang = next;
}

/** The string for `key` with `{name}` placeholders filled in. */
export function t(key: Key, args: Record<string, string | number> = {}): string {
  const template = tables[lang][key] ?? tables.en[key] ?? key;
  return template.replace(/\{(\w+)\}/g, (_, name: string) => String(args[name] ?? `{${name}}`));
}

/** Fills every `[data-i18n]` element with its string. */
export function applyStatic(root: ParentNode = document): void {
  root.querySelectorAll<HTMLElement>("[data-i18n]").forEach((el) => {
    el.textContent = t(el.dataset.i18n as Key);
  });
}

/** `m:ss` or `h:mm:ss`, like the tray tooltip. */
export function clock(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor(secs / 60) % 60;
  const s = String(secs % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}
