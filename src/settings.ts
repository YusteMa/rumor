// The Settings window (SPEC §4.4): simple outside, complete inside. Every
// change applies at once; the window never holds state of its own, it shows
// the `SettingsView` Rust sends and asks Rust to change it.

import type { Color } from "./api";
import { applyStatic, setLang, t } from "./i18n";
import {
  settingsBackend,
  type Action,
  type Endpoint,
  type Patch,
  type Settings,
  type SettingsView,
  type ShortcutError,
} from "./settings-api";
import { applyAccent } from "./ui/accent";

type Key = Parameters<typeof t>[0];

const api = await settingsBackend;
const side = document.getElementById("side") as HTMLElement;
const content = document.getElementById("content") as HTMLElement;

let view: SettingsView | null = null;
let syncs: ((v: SettingsView) => void)[] = [];
let current = "sound";
let lang: string | null = null;

// ---- Building blocks ---------------------------------------------------------

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]> & { className?: string } = {},
  ...children: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children);
  return node;
}

function send(patch: Patch<Settings>): void {
  void api.update(patch).then((v) => v && render(v));
}

interface Row {
  node: HTMLElement;
  error: HTMLElement;
}

/** `stack` puts the control under the text, full width (sliders, long lists). */
function row(label: Key, hint: Key | null, control: HTMLElement, stack = false): Row {
  // The label points at the real control, even inside a wrapper.
  const target = control.matches("input, select, button")
    ? control
    : (control.querySelector("input, select, button") ?? control);
  target.id ||= `c-${Math.random().toString(36).slice(2)}`;
  const error = el("span", { className: "row-error", hidden: true });
  error.setAttribute("role", "alert");
  const text = el(
    "div",
    { className: "row-text" },
    el("label", { className: "row-label", htmlFor: target.id, textContent: t(label) }),
    ...(hint ? [el("span", { className: "row-hint", textContent: t(hint) })] : []),
    error,
  );
  const node = el("div", { className: `row${stack ? " row-stack" : ""}` }, text, el("div", { className: "row-control" }, control));
  return { node, error };
}

function card(...rows: Row[]): HTMLElement {
  return el("div", { className: "card" }, ...rows.map((r) => r.node));
}

function select<T extends string>(
  options: [T, string][],
  get: (s: Settings) => T,
  set: (v: T) => Patch<Settings>,
): HTMLSelectElement {
  const node = el("select", { className: "select" });
  for (const [value, label] of options) node.append(el("option", { value, textContent: label }));
  node.addEventListener("change", () => send(set(node.value as T)));
  syncs.push((v) => {
    node.value = get(v.settings);
  });
  return node;
}

function toggle(get: (s: Settings) => boolean, set: (on: boolean) => Patch<Settings>): HTMLButtonElement {
  const node = el("button", { className: "switch", type: "button" });
  node.setAttribute("role", "switch");
  node.addEventListener("click", () => send(set(node.getAttribute("aria-checked") !== "true")));
  syncs.push((v) => node.setAttribute("aria-checked", String(get(v.settings))));
  return node;
}

/** A slider with its value next to it; sends at most once per frame. */
function slider(opts: {
  min: number;
  max: number;
  step: number;
  get: (s: Settings) => number;
  set: (v: number) => Patch<Settings>;
  format: (v: number) => string;
}): HTMLElement {
  const input = el("input", { className: "slider", type: "range", min: String(opts.min), max: String(opts.max), step: String(opts.step) });
  const value = el("span", { className: "row-value" });
  let dragging = false;
  let raf = 0;
  const paint = () => {
    const v = input.valueAsNumber;
    input.style.setProperty("--fill", `${((v - opts.min) / (opts.max - opts.min)) * 100}%`);
    value.textContent = opts.format(v);
  };
  input.addEventListener("input", () => {
    paint();
    raf ||= requestAnimationFrame(() => {
      raf = 0;
      send(opts.set(input.valueAsNumber));
    });
  });
  input.addEventListener("pointerdown", () => (dragging = true));
  for (const end of ["pointerup", "pointercancel", "change"]) input.addEventListener(end, () => (dragging = false));
  syncs.push((v) => {
    if (dragging) return;
    input.value = String(opts.get(v.settings));
    paint();
  });
  return el("span", { className: "inline" }, input, value);
}

function button(label: Key, onClick: () => void, primary = false): HTMLButtonElement {
  const node = el("button", { className: `btn${primary ? " primary" : ""}`, type: "button", textContent: t(label) });
  node.addEventListener("click", onClick);
  return node;
}

function status(): { node: HTMLElement; say: (text: string) => void } {
  const node = el("p", { className: "status" });
  node.setAttribute("role", "status");
  return { node, say: (text) => (node.textContent = text) };
}

// ---- Shortcuts -----------------------------------------------------------------

/** "Ctrl+Alt+P" from a key event, in the format Rust parses; `null` = modifier only. */
function comboOf(e: KeyboardEvent): string | null {
  const code = e.code;
  if (/^(Control|Alt|Shift|Meta|OS)(Left|Right)?$/.test(code) || code === "") return null;
  const key = code.startsWith("Key") ? code.slice(3) : code.startsWith("Digit") ? code.slice(5) : code;
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean);
  return [...mods, key].join("+");
}

/** For display: "Super" is the Windows key. */
const pretty = (combo: string) => combo.replace("Super", "Win").replaceAll("+", " + ");

const ERRORS: Record<ShortcutError["kind"], Key> = {
  invalid: "errInvalid",
  noModifier: "errNoModifier",
  typesCharacter: "errTypesCharacter",
  duplicate: "errDuplicate",
  taken: "errTaken",
};

function shortcutRow(action: Action, label: Key): Row {
  const btn = el("button", { className: "shortcut-btn", type: "button" });
  const r = row(label, null, btn);
  let recording = false;
  const show = (v: SettingsView) => {
    const s = v.shortcuts.find((x) => x.action === action);
    const active = s?.active ?? null;
    btn.textContent = recording ? t("shortcutRecording") : active ? pretty(active) : t("shortcutNone");
    btn.classList.toggle("is-empty", !active && !recording);
    btn.classList.toggle("is-recording", recording);
    if (!recording && s?.unavailable && !active) {
      r.error.hidden = false;
      r.error.textContent = t("shortcutUnavailable", { label: pretty(s.unavailable) });
    }
  };
  const stop = () => {
    recording = false;
    window.removeEventListener("keydown", onKey, true);
    if (view) show(view);
  };
  const onKey = (e: KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") return stop();
    const clear = (e.key === "Delete" || e.key === "Backspace") && !e.ctrlKey && !e.altKey && !e.metaKey;
    const combo = clear ? null : comboOf(e);
    if (!clear && combo === null) return; // still holding modifiers
    recording = false;
    window.removeEventListener("keydown", onKey, true);
    api
      .setShortcut(action, combo)
      .then((v) => {
        r.error.hidden = true;
        if (v) render(v);
      })
      .catch((err: ShortcutError) => {
        r.error.hidden = false;
        r.error.textContent = t(ERRORS[err?.kind] ?? "errInvalid");
        if (view) show(view);
      });
  };
  btn.addEventListener("click", () => {
    if (recording) return stop();
    recording = true;
    r.error.hidden = true;
    if (view) show(view);
    window.addEventListener("keydown", onKey, true);
  });
  btn.addEventListener("blur", () => recording && stop());
  syncs.push(show);
  return r;
}

// ---- Sections --------------------------------------------------------------------

const colors = (): [Color, string][] => [
  ["pink", t("colorPink")],
  ["brown", t("colorBrown")],
  ["white", t("colorWhite")],
];

const SECTIONS: { id: string; title: Key; build: () => Node[] }[] = [
  {
    id: "sound",
    title: "secSound",
    build: () => [
      card(
        row("optStartColor", "hintStartColor", select([["last", t("startLast")], ...colors()], (s) => s.sound.startColor, (v) => ({ sound: { startColor: v } }))),
        row("optFade", "hintFade", slider({ min: 0.2, max: 3, step: 0.1, get: (s) => s.sound.fadeMs / 1000, set: (v) => ({ sound: { fadeMs: Math.round(v * 1000) } }), format: (v) => t("seconds", { n: v.toLocaleString(lang ?? "es", { minimumFractionDigits: 1, maximumFractionDigits: 1 }) }) }), true),
        row("optMaxVolume", "hintMaxVolume", slider({ min: 10, max: 100, step: 5, get: (s) => Math.round(s.sound.maxVolume * 100), set: (v) => ({ sound: { maxVolume: v / 100 } }), format: (v) => t("percent", { n: v }) }), true),
        row("optPerOutputVolume", "hintPerOutputVolume", toggle((s) => s.sound.perOutputVolume, (on) => ({ sound: { perOutputVolume: on } }))),
        row("optMono", "hintMono", toggle((s) => s.sound.mono, (on) => ({ sound: { mono: on } }))),
      ),
    ],
  },
  {
    id: "playback",
    title: "secPlayback",
    build: () => [
      card(
        row("optOutputChange", "hintOutputChange", select([["pauseOnSpeakers", t("policyPauseOnSpeakers")], ["pauseAlways", t("policyPauseAlways")], ["keepPlaying", t("policyKeepPlaying")]], (s) => s.playback.onOutputChange, (v) => ({ playback: { onOutputChange: v } }))),
        row("optResume", "hintResume", toggle((s) => s.playback.resumeOnReconnect, (on) => ({ playback: { resumeOnReconnect: on } }))),
        row("optOnLock", "hintOnLock", select([["pauseResumeWithHeadphones", t("lockPauseResume")], ["pause", t("lockPause")], ["nothing", t("lockNothing")]], (s) => s.playback.onLock, (v) => ({ playback: { onLock: v } })), true),
        row("optOnLaunch", "hintOnLaunch", select([["silent", t("launchSilent")], ["play", t("launchPlay")], ["resume", t("launchResume")]], (s) => s.playback.onLaunch, (v) => ({ playback: { onLaunch: v } }))),
      ),
    ],
  },
  {
    id: "timer",
    title: "secTimer",
    build: () => {
      const inputs = [0, 1, 2].map((i) => {
        const input = el("input", { className: "number", type: "number", min: "1", max: "600", step: "1" });
        input.setAttribute("aria-label", `${t("optPresets")} ${i + 1}`);
        input.addEventListener("change", () => {
          if (!view) return;
          const presets = [...view.settings.timer.presets] as [number, number, number];
          presets[i] = Math.round(input.valueAsNumber) || presets[i];
          send({ timer: { presets } });
        });
        syncs.push((v) => {
          if (document.activeElement !== input) input.value = String(v.settings.timer.presets[i]);
        });
        return input;
      });
      const group = el("span", { className: "inline" }, ...inputs, el("span", { className: "row-value", textContent: t("minutesUnit") }));
      return [
        card(
          row("optPresets", "hintPresets", group),
          row("optEndFade", "hintEndFade", slider({ min: 2, max: 60, step: 1, get: (s) => s.timer.endFadeS, set: (v) => ({ timer: { endFadeS: v } }), format: (v) => t("seconds", { n: v }) }), true),
          row("optNotifyEnd", "hintNotifyEnd", toggle((s) => s.timer.notifyOnEnd, (on) => ({ timer: { notifyOnEnd: on } }))),
        ),
      ];
    },
  },
  {
    id: "shortcuts",
    title: "secShortcuts",
    build: () => [
      el("p", { className: "intro", textContent: t("shortcutsIntro") }),
      card(
        shortcutRow("toggle", "actToggle"),
        shortcutRow("volumeUp", "actVolumeUp"),
        shortcutRow("volumeDown", "actVolumeDown"),
        shortcutRow("nextColor", "actNextColor"),
        shortcutRow("openPanel", "actOpenPanel"),
      ),
    ],
  },
  {
    id: "tray",
    title: "secTray",
    build: () => [
      card(
        row("optLeftClick", "hintLeftClick", select([["toggle", t("leftToggle")], ["panel", t("leftPanel")]], (s) => s.tray.leftClick, (v) => ({ tray: { leftClick: v } }))),
        row("optShowRemaining", "hintShowRemaining", toggle((s) => s.tray.showRemaining, (on) => ({ tray: { showRemaining: on } }))),
      ),
    ],
  },
  {
    id: "appearance",
    title: "secAppearance",
    build: () => [
      card(
        row("optTheme", "hintTheme", select([["system", t("themeSystem")], ["light", t("themeLight")], ["dark", t("themeDark")]], (s) => s.appearance.theme, (v) => ({ appearance: { theme: v } }))),
        row("optAccent", "hintAccent", select([["noise", t("accentNoise")], ["system", t("accentSystem")]], (s) => s.appearance.accent, (v) => ({ appearance: { accent: v } }))),
        row("optMica", "hintMica", toggle((s) => s.appearance.mica, (on) => ({ appearance: { mica: on } }))),
        row("optSpectrum", "hintSpectrum", toggle((s) => s.appearance.spectrum, (on) => ({ appearance: { spectrum: on } }))),
        row("optReduceMotion", "hintReduceMotion", toggle((s) => s.appearance.reduceMotion, (on) => ({ appearance: { reduceMotion: on } }))),
        row("optLanguage", "hintLanguage", select([["auto", t("langAuto")], ["es", t("langEs")], ["en", t("langEn")]], (s) => s.appearance.language, (v) => ({ appearance: { language: v } }))),
      ),
    ],
  },
  {
    id: "startup",
    title: "secStartup",
    build: () => [card(row("optLaunchAtLogin", "hintLaunchAtLogin", toggle((s) => s.startup.launchAtLogin, (on) => ({ startup: { launchAtLogin: on } }))))],
  },
  {
    id: "advanced",
    title: "secAdvanced",
    build: () => {
      const device = el("select", { className: "select" });
      const fill = (outputs: Endpoint[]) => {
        device.replaceChildren(el("option", { value: "", textContent: t("deviceDefault") }), ...outputs.map((o) => el("option", { value: o.id, textContent: o.name })));
        if (view) device.value = view.settings.advanced.outputDevice ?? "";
      };
      void api.listOutputs().then(fill);
      device.addEventListener("change", () => send({ advanced: { outputDevice: device.value || null } }));
      syncs.push((v) => {
        device.value = v.settings.advanced.outputDevice ?? "";
      });

      const info = el("span", { className: "row-value selectable" });
      syncs.push((v) => {
        const o = v.output;
        info.textContent = o
          ? t("deviceInfo", { name: o.endpoint.name, rate: o.sampleRate.toLocaleString(lang ?? "es"), channels: o.channels, format: o.sampleFormat })
          : t("deviceNone");
      });

      const result = status();
      const backup = el(
        "span",
        { className: "inline" },
        button("btnExport", () => void api.exportSettings().then((ok) => ok && result.say(t("exportOk")))),
        button("btnImport", () =>
          void api
            .importSettings()
            .then(async (ok) => {
              if (!ok) return;
              result.say(t("importOk"));
              const v = await api.get();
              if (v) render(v);
            })
            .catch((e) => result.say(t("importFail", { error: String(e) }))),
        ),
      );

      const resetZone = el("span", { className: "inline" });
      const askReset = () =>
        resetZone.replaceChildren(
          button("btnCancel", () => resetZone.replaceChildren(resetButton)),
          button(
            "btnResetConfirm",
            () =>
              void api.reset().then((v) => {
                resetZone.replaceChildren(resetButton);
                result.say(t("resetOk"));
                if (v) render(v);
              }),
            true,
          ),
        );
      const resetButton = button("btnReset", askReset);
      resetZone.append(resetButton);

      return [
        card(
          row("optDevice", "hintDevice", device, true),
          row("optBuffer", "hintBuffer", select([["auto", t("bufferAuto")], ["ms10", t("bufferMs", { n: 10 })], ["ms20", t("bufferMs", { n: 20 })], ["ms50", t("bufferMs", { n: 50 })]], (s) => s.advanced.buffer, (v) => ({ advanced: { buffer: v } }))),
          row("optDeviceInfo", null, info, true),
        ),
        el("div", { className: "spacer" }),
        card(row("optBackup", "hintBackup", backup), row("optLogs", "hintLogs", button("btnLogs", () => void api.openLogs())), row("optReset", "hintReset", resetZone)),
        result.node,
      ];
    },
  },
  {
    id: "about",
    title: "secAbout",
    build: () => {
      const version = el("p", { className: "muted" });
      syncs.push((v) => (version.textContent = t("aboutVersion", { v: v.version })));
      return [
        el(
          "div",
          { className: "card about" },
          el("h2", { className: "about-name", textContent: "Rumor" }),
          el("p", { textContent: t("aboutTagline") }),
          version,
          el("p", {}, `${t("aboutRepo")}: `, el("span", { className: "selectable", textContent: "github.com/YusteMa/rumor" })),
          el("p", { className: "muted", textContent: t("aboutLicense") }),
          el("p", { textContent: t("aboutTelemetry") }),
          el("p", { className: "muted", textContent: t("aboutBugs") }),
        ),
      ];
    },
  },
];

// ---- Rendering --------------------------------------------------------------------

function buildSide(): void {
  side.replaceChildren(
    ...SECTIONS.map((s) => {
      const item = el("button", { className: "side-item", type: "button", textContent: t(s.title) });
      if (s.id === current) item.setAttribute("aria-current", "page");
      item.addEventListener("click", () => {
        current = s.id;
        buildSide();
        buildContent();
        content.focus();
      });
      return item;
    }),
  );
}

function buildContent(): void {
  syncs = [];
  const section = SECTIONS.find((s) => s.id === current) ?? SECTIONS[0];
  content.replaceChildren(el("h1", { className: "section-title", textContent: t(section.title) }), ...section.build());
  if (view) for (const sync of syncs) sync(view);
}

function render(next: SettingsView): void {
  view = next;
  const root = document.documentElement;
  root.toggleAttribute("data-mica", next.mica);
  root.dataset.noise = next.settings.last.color;
  root.toggleAttribute("data-reduce-motion", next.settings.appearance.reduceMotion);
  applyAccent(next.settings.appearance.accent === "system" ? next.systemAccent : null);
  if (next.lang !== lang) {
    lang = next.lang;
    setLang(next.lang);
    applyStatic();
    buildSide();
    buildContent();
    return;
  }
  for (const sync of syncs) sync(next);
}

api.onChanged(render);
const initial = await api.get();
if (initial) render(initial);
void api.ready();
