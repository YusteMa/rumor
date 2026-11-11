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
import { dropdown } from "./ui/dropdown";

type Key = Parameters<typeof t>[0];

const api = await settingsBackend;
const side = document.getElementById("side") as HTMLElement;
const content = document.getElementById("content") as HTMLElement;

let view: SettingsView | null = null;
let syncs: ((v: SettingsView) => void)[] = [];
let current = "general";
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
  const target = control.matches("input, button")
    ? control
    : (control.querySelector("input, button") ?? control);
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
): HTMLButtonElement {
  const dd = dropdown(options, (v) => send(set(v)));
  syncs.push((v) => dd.setValue(get(v.settings)));
  return dd.node;
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

// ---- More building blocks -----------------------------------------------------------

/** A heading inside a section (Advanced groups its cards under them). */
function subtitle(key: Key): HTMLElement {
  return el("h2", { className: "subtitle", textContent: t(key) });
}

/** A whole-number input that sends on change and clamps to its range. */
function numberInput(opts: {
  min: number;
  max: number;
  label: string;
  get: (s: Settings) => number;
  set: (v: number) => Patch<Settings>;
}): HTMLInputElement {
  const input = el("input", { className: "number", type: "number", min: String(opts.min), max: String(opts.max), step: "1" });
  input.setAttribute("aria-label", opts.label);
  input.addEventListener("change", () => {
    if (!view) return;
    const v = Math.round(input.valueAsNumber);
    if (Number.isFinite(v)) send(opts.set(Math.min(opts.max, Math.max(opts.min, v))));
    else input.value = String(opts.get(view.settings));
  });
  syncs.push((v) => {
    if (document.activeElement !== input) input.value = String(opts.get(v.settings));
  });
  return input;
}

const colors = (): [Color, string][] => [
  ["pink", t("colorPink")],
  ["brown", t("colorBrown")],
  ["white", t("colorWhite")],
];

/** "Si se desconectan los cascos" joins the policy and "resume on reconnect". */
type OutputChoice = "pause" | "pauseResume" | "pauseAlways" | "keepPlaying";

function outputChoice(s: Settings): OutputChoice {
  const p = s.playback;
  if (p.onOutputChange === "pauseOnSpeakers") return p.resumeOnReconnect ? "pauseResume" : "pause";
  return p.onOutputChange;
}

function outputPatch(choice: OutputChoice): Patch<Settings> {
  switch (choice) {
    case "pause":
      return { playback: { onOutputChange: "pauseOnSpeakers", resumeOnReconnect: false } };
    case "pauseResume":
      return { playback: { onOutputChange: "pauseOnSpeakers", resumeOnReconnect: true } };
    default:
      return { playback: { onOutputChange: choice, resumeOnReconnect: false } };
  }
}

// ---- Tags ----------------------------------------------------------------------------

const TAG_COLORS = ["rose", "amber", "sage", "sky", "lilac", "stone"] as const;
const TAG_COLOR_NAMES: Record<(typeof TAG_COLORS)[number], Key> = {
  rose: "tagRose",
  amber: "tagAmber",
  sage: "tagSage",
  sky: "tagSky",
  lilac: "tagLilac",
  stone: "tagStone",
};

/** The colour after `color`, so a click on the dot walks the palette. */
function nextColor(color: string): string {
  const i = TAG_COLORS.indexOf(color as (typeof TAG_COLORS)[number]);
  return TAG_COLORS[(i + 1) % TAG_COLORS.length];
}

function tagError(e: unknown): Key {
  return String(e).includes("already") ? "tagErrDuplicate" : "tagErrEmpty";
}

/** Settings → Study → Tags: rename in place, a click on the dot recolours,
 *  × deletes (the sessions stay, untagged). */
function tagEditor(): Row {
  const list = el("div", { className: "tags" });
  const name = el("input", { className: "text", type: "text", placeholder: t("tagPlaceholder"), maxLength: 40 });
  name.setAttribute("aria-label", t("tagPlaceholder"));
  const add = button("btnAdd", () => void create());
  const adder = el("div", { className: "tag-add" }, name, add);
  const editor = el("div", { className: "tag-editor" }, list, adder);
  const r = row("optTags", "hintTags", editor, true);
  const fail = (e: unknown) => {
    r.error.hidden = false;
    r.error.textContent = t(tagError(e));
  };
  const create = async () => {
    const used = new Set(view?.tags.map((g) => g.color));
    const color = TAG_COLORS.find((c) => !used.has(c)) ?? TAG_COLORS[(view?.tags.length ?? 0) % TAG_COLORS.length];
    try {
      await api.upsertTag(null, name.value, color);
      name.value = "";
      r.error.hidden = true;
    } catch (e) {
      fail(e);
    }
  };
  name.addEventListener("keydown", (e) => {
    if (e.key === "Enter") void create();
  });

  let shown = "";
  syncs.push((v) => {
    const key = v.tags.map((g) => `${g.id}:${g.name}:${g.color}`).join("|");
    // Don't rebuild under the user's typing.
    if (key === shown || list.contains(document.activeElement)) return;
    shown = key;
    list.replaceChildren(
      ...v.tags.map((tag) => {
        const dot = el("button", { className: "tag-dot", type: "button" });
        dot.style.setProperty("--tag", `var(--tag-${tag.color})`);
        const colorName = t(TAG_COLOR_NAMES[tag.color as (typeof TAG_COLORS)[number]] ?? "tagRose");
        dot.setAttribute("aria-label", t("tagColor", { name: tag.name, color: colorName }));
        dot.addEventListener("click", () => void api.upsertTag(tag.id, tag.name, nextColor(tag.color)).catch(fail));
        const input = el("input", { className: "text", type: "text", value: tag.name, maxLength: 40 });
        input.setAttribute("aria-label", t("tagRename", { name: tag.name }));
        input.addEventListener("change", () =>
          void api
            .upsertTag(tag.id, input.value, tag.color)
            .then(() => (r.error.hidden = true))
            .catch((e) => {
              input.value = tag.name;
              fail(e);
            }),
        );
        input.addEventListener("keydown", (e) => {
          if (e.key === "Enter") input.blur();
          if (e.key === "Escape") {
            input.value = tag.name;
            input.blur();
          }
        });
        const remove = el("button", { className: "icon-button", type: "button", innerHTML: CROSS });
        remove.setAttribute("aria-label", t("tagDelete", { name: tag.name }));
        remove.title = t("tagDelete", { name: tag.name });
        remove.addEventListener("click", () => void api.deleteTag(tag.id));
        return el("div", { className: "tag-row" }, dot, input, remove);
      }),
    );
  });
  return r;
}

const CROSS =
  '<svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4.5 4.5l7 7M11.5 4.5l-7 7" fill="none" stroke="currentColor" stroke-linecap="round"/></svg>';

// ---- Sections --------------------------------------------------------------------

/** "Modo estudio" is on: the rest of Study, and its shortcut, appear. */
const studyOn = () => view?.settings.study.enabled ?? false;

const SECTIONS: { id: string; title: Key; build: () => Node[] }[] = [
  {
    id: "general",
    title: "secGeneral",
    build: () => [
      card(
        row("optOutputChange", "hintOutputChange", select<OutputChoice>([["pause", t("policyPause")], ["pauseResume", t("policyPauseResume")], ["pauseAlways", t("policyPauseAlways")], ["keepPlaying", t("policyKeepPlaying")]], outputChoice, outputPatch), true),
        row("optOnLock", "hintOnLock", select([["pauseResumeWithHeadphones", t("lockPauseResume")], ["pause", t("lockPause")], ["nothing", t("lockNothing")]], (s) => s.playback.onLock, (v) => ({ playback: { onLock: v } })), true),
        row("optAlerts", "hintAlerts", select([["bell", t("alertsBell")], ["notification", t("alertsNotification")], ["both", t("alertsBoth")], ["none", t("alertsNone")]], (s) => s.general.alerts, (v) => ({ general: { alerts: v } }))),
        row("optEarBreak", "hintEarBreak", select([["0", t("earOff")], ...[2, 3, 4].map((n): [string, string] => [String(n), t("earEvery", { n })])], (s) => String(s.general.earBreakHours), (v) => ({ general: { earBreakHours: Number(v) } }))),
        row("optMono", "hintMono", toggle((s) => s.sound.mono, (on) => ({ sound: { mono: on } }))),
        row("optOnLaunch", "hintOnLaunch", select([["silent", t("launchSilent")], ["play", t("launchPlay")], ["resume", t("launchResume")]], (s) => s.playback.onLaunch, (v) => ({ playback: { onLaunch: v } }))),
        row("optLaunchAtLogin", "hintLaunchAtLogin", toggle((s) => s.startup.launchAtLogin, (on) => ({ startup: { launchAtLogin: on } }))),
      ),
    ],
  },
  {
    id: "study",
    title: "secStudy",
    build: () => {
      const main = card(row("optStudyMode", "hintStudyMode", toggle((s) => s.study.enabled, (on) => ({ study: { enabled: on } }))));
      if (!studyOn()) return [main];
      const times = el(
        "div",
        { className: "pomodoro" },
        ...(
          [
            ["pomoWork", 5, 180, (s: Settings) => s.study.workMin, (v: number) => ({ study: { workMin: v } })],
            ["pomoBreak", 1, 60, (s: Settings) => s.study.shortBreakMin, (v: number) => ({ study: { shortBreakMin: v } })],
            ["pomoLong", 1, 90, (s: Settings) => s.study.longBreakMin, (v: number) => ({ study: { longBreakMin: v } })],
            ["pomoCycles", 1, 12, (s: Settings) => s.study.cycles, (v: number) => ({ study: { cycles: v } })],
          ] as const
        ).map(([label, min, max, get, set]) =>
          el("label", { className: "pomodoro-field" }, el("span", { textContent: t(label) }), numberInput({ min, max, label: t(label), get, set })),
        ),
      );
      return [
        main,
        card(
          row("optPomodoro", "hintPomodoro", times, true),
          row("optBreakNoise", "hintBreakNoise", select([["half", t("breakHalf")], ["pause", t("breakPause")], ["keep", t("breakKeep")]], (s) => s.study.breakNoise, (v) => ({ study: { breakNoise: v } }))),
          row("optLink", "hintLink", toggle((s) => s.study.linkNoise, (on) => ({ study: { linkNoise: on } }))),
          row("optWeeklyGoal", "hintWeeklyGoal", slider({ min: 0, max: 80, step: 1, get: (s) => s.study.weeklyGoalHours, set: (v) => ({ study: { weeklyGoalHours: v } }), format: (v) => (v === 0 ? t("goalOff") : t("goalHours", { n: v })) }), true),
          row("optIdle", "hintIdle", select([["0", t("idleOff")], ...[10, 15, 20, 30].map((n): [string, string] => [String(n), t("idleAfter", { n })])], (s) => String(s.study.idleMinutes), (v) => ({ study: { idleMinutes: Number(v) } }))),
        ),
        card(tagEditor()),
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
        ...(studyOn() ? [shortcutRow("study", "actStudy")] : []),
      ),
      card(row("optLeftClick", "hintLeftClick", select([["toggle", t("leftToggle")], ["panel", t("leftPanel")]], (s) => s.tray.leftClick, (v) => ({ tray: { leftClick: v } })))),
    ],
  },
  {
    id: "appearance",
    title: "secAppearance",
    build: () => [
      card(
        row("optTheme", "hintTheme", select([["system", t("themeSystem")], ["light", t("themeLight")], ["dark", t("themeDark")]], (s) => s.appearance.theme, (v) => ({ appearance: { theme: v } }))),
        row("optAccent", "hintAccent", select([["noise", t("accentNoise")], ["system", t("accentSystem")]], (s) => s.appearance.accent, (v) => ({ appearance: { accent: v } }))),
        row("optSpectrum", "hintSpectrum", toggle((s) => s.appearance.spectrum, (on) => ({ appearance: { spectrum: on } }))),
        row("optLanguage", "hintLanguage", select([["auto", t("langAuto")], ["es", t("langEs")], ["en", t("langEn")]], (s) => s.appearance.language, (v) => ({ appearance: { language: v } }))),
      ),
    ],
  },
  {
    id: "advanced",
    title: "secAdvanced",
    build: () => {
      // "" is Windows' default output.
      const device = dropdown<string>([["", t("deviceDefault")]], (id) => send({ advanced: { outputDevice: id || null } }));
      void api.listOutputs().then((outputs: Endpoint[]) => {
        device.setOptions([["", t("deviceDefault")], ...outputs.map((o): [string, string] => [o.id, o.name])]);
        if (view) device.setValue(view.settings.advanced.outputDevice ?? "");
      });
      syncs.push((v) => device.setValue(v.settings.advanced.outputDevice ?? ""));

      const info = el("span", { className: "row-value selectable" });
      syncs.push((v) => {
        const o = v.output;
        info.textContent = o
          ? t("deviceInfo", { name: o.endpoint.name, rate: o.sampleRate.toLocaleString(lang ?? "es"), channels: o.channels, format: o.sampleFormat })
          : t("deviceNone");
      });

      const presets = el(
        "span",
        { className: "inline" },
        ...[0, 1, 2].map((i) =>
          numberInput({
            min: 1,
            max: 600,
            label: `${t("optPresets")} ${i + 1}`,
            get: (s) => s.timer.presets[i],
            set: (v) => {
              const p = [...(view?.settings.timer.presets ?? [25, 50, 90])] as [number, number, number];
              p[i] = v;
              return { timer: { presets: p } };
            },
          }),
        ),
        el("span", { className: "row-value", textContent: t("minutesUnit") }),
      );

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
        subtitle("subSound"),
        card(
          row("optStartColor", "hintStartColor", select([["last", t("startLast")], ...colors()], (s) => s.sound.startColor, (v) => ({ sound: { startColor: v } }))),
          row("optFade", "hintFade", slider({ min: 0.2, max: 3, step: 0.1, get: (s) => s.sound.fadeMs / 1000, set: (v) => ({ sound: { fadeMs: Math.round(v * 1000) } }), format: (v) => t("seconds", { n: v.toLocaleString(lang ?? "es", { minimumFractionDigits: 1, maximumFractionDigits: 1 }) }) }), true),
          row("optMaxVolume", "hintMaxVolume", slider({ min: 10, max: 100, step: 5, get: (s) => Math.round(s.sound.maxVolume * 100), set: (v) => ({ sound: { maxVolume: v / 100 } }), format: (v) => t("percent", { n: v }) }), true),
          row("optPerOutputVolume", "hintPerOutputVolume", toggle((s) => s.sound.perOutputVolume, (on) => ({ sound: { perOutputVolume: on } }))),
        ),
        subtitle("subTimer"),
        card(
          row("optPresets", "hintPresets", presets),
          row("optEndFade", "hintEndFade", slider({ min: 2, max: 60, step: 1, get: (s) => s.timer.endFadeS, set: (v) => ({ timer: { endFadeS: v } }), format: (v) => t("seconds", { n: v }) }), true),
        ),
        subtitle("subOutput"),
        card(
          row("optDevice", "hintDevice", device.node, true),
          row("optBuffer", "hintBuffer", select([["auto", t("bufferAuto", { n: 80 })], ["ms20", t("bufferMs", { n: 20 })], ["ms50", t("bufferMs", { n: 50 })], ["ms100", t("bufferMs", { n: 100 })]], (s) => s.advanced.buffer, (v) => ({ advanced: { buffer: v } }))),
          row("optDeviceInfo", null, info, true),
        ),
        subtitle("subWindow"),
        card(row("optMica", "hintMica", toggle((s) => s.appearance.mica, (on) => ({ appearance: { mica: on } })))),
        subtitle("subMaintenance"),
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
      // One button: "Buscar ahora", or "Actualizar" once a version is found.
      const result = status();
      let checking = false;
      const action = button("btnCheckNow", () => {
        if (view?.update) {
          void api.installUpdate().catch(() => result.say(t("updateFailed")));
          return;
        }
        checking = true;
        result.say("");
        if (view) syncUpdate(view);
        void api
          .checkUpdate()
          .then((v) => !v && result.say(t("updateLatest")))
          .catch(() => result.say(t("updateCheckFail")))
          .finally(() => {
            checking = false;
            if (view) syncUpdate(view);
          });
      });
      const syncUpdate = (v: SettingsView) => {
        const u = v.update;
        action.textContent = t(
          u ? (u.phase === "installing" ? "updateInstalling" : u.phase === "failed" ? "updateRetry" : "updateNow") : checking ? "updateChecking" : "btnCheckNow",
        );
        action.classList.toggle("primary", !!u && u.phase !== "installing");
        action.disabled = checking || u?.phase === "installing";
        if (u?.phase === "available") result.say(t("updateFound", { v: u.version }));
        if (u?.phase === "failed") result.say(t("updateFailed"));
      };
      syncs.push(syncUpdate);
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
        subtitle("subUpdates"),
        card(
          row("optCheckUpdates", "hintCheckUpdates", toggle((s) => s.startup.checkUpdates, (on) => ({ startup: { checkUpdates: on } }))),
          row("optUpdateNow", "hintUpdateNow", action),
        ),
        result.node,
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

/** Sections whose rows depend on "Modo estudio". */
const STUDY_DEPENDENT = ["study", "shortcuts"];

function render(next: SettingsView): void {
  const studyChanged = view !== null && view.settings.study.enabled !== next.settings.study.enabled;
  view = next;
  const root = document.documentElement;
  root.toggleAttribute("data-mica", next.mica);
  root.dataset.noise = next.settings.last.color;
  applyAccent(next.settings.appearance.accent === "system" ? next.systemAccent : null);
  if (next.lang !== lang) {
    lang = next.lang;
    setLang(next.lang);
    applyStatic();
    buildSide();
    buildContent();
    return;
  }
  if (studyChanged && STUDY_DEPENDENT.includes(current)) {
    buildContent();
    return;
  }
  for (const sync of syncs) sync(next);
}

api.onChanged(render);
const initial = await api.get();
if (initial) render(initial);
void api.ready();
