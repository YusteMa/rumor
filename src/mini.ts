// The mini clock (SPEC §4.3b): the session's time and its play/pause, in a
// small window that floats over everything. Rust decides when it exists;
// this page draws the study state it is sent (CLAUDE.md rule 3). Outside
// Tauri it runs on the panel's mock, for design work (`?study=…&run=…`).

import { backend, isTauri, type PanelState, type StudyView } from "./api";
import { applyStatic, setLang, t } from "./i18n";
import { applyAccent } from "./ui/accent";
import { dial } from "./ui/dial";
import { face } from "./ui/study";

const $ = (id: string) => document.getElementById(id) as HTMLElement;
const root = $("mini");
const toggle = $("mini-toggle") as HTMLButtonElement;
const time = $("mini-time");
const sub = $("mini-sub");
const close = $("mini-close") as HTMLButtonElement;
const ring = dial($("mini-dial"), "mini");

interface MiniBackend {
  getState(): Promise<PanelState | null>;
  toggle(): Promise<void>;
  dismiss(): Promise<void>;
  openPanel(): Promise<void>;
  ready(): Promise<void>;
  drag(): Promise<void>;
  onState(cb: (s: PanelState) => void): void;
  onTick(cb: (s: StudyView) => void): void;
  onShown(cb: () => void): void;
  onHiding(cb: () => void): void;
}

async function tauriBackend(): Promise<MiniBackend> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const call = (cmd: string) => invoke<void>(cmd).catch((e) => console.error(cmd, e));
  return {
    getState: () => invoke<PanelState | null>("get_state"),
    toggle: () => call("study_toggle"),
    dismiss: () => call("mini_dismiss"),
    openPanel: () => call("mini_open_panel"),
    ready: () => call("mini_ready"),
    drag: () => getCurrentWindow().startDragging(),
    onState: (cb) => void listen<PanelState>("state-changed", (e) => cb(e.payload)),
    onTick: (cb) => void listen<StudyView>("study-tick", (e) => cb(e.payload)),
    onShown: (cb) => void listen("mini-shown", () => cb()),
    onHiding: (cb) => void listen("mini-hiding", () => cb()),
  };
}

async function mockBackend(): Promise<MiniBackend> {
  const panel = await backend;
  return {
    getState: () => panel.getState(),
    toggle: () => panel.studyToggle(),
    dismiss: async () => void document.body.classList.replace("is-open", "is-closing"),
    openPanel: async () => {},
    ready: async () => {},
    drag: async () => {},
    onState: (cb) => panel.onState(cb),
    onTick: () => {},
    onShown: () => {},
    onHiding: () => {},
  };
}

const api = await (isTauri ? tauriBackend() : mockBackend());
let lang: string | null = null;

function renderStudy(s: StudyView): void {
  const f = face(s);
  ring.render(f);
  root.classList.toggle("is-running", s.run === "running");
  time.textContent = f.time;
  // In a few words: the phase, "En pausa", or the mode.
  sub.textContent =
    s.run === "paused"
      ? t("studyPaused")
      : s.phaseLeftSecs !== null
        ? s.phase === "work"
          ? t("phaseWork", { n: s.cycle, total: s.cycles })
          : f.sub
        : t("modeStopwatch");
  toggle.setAttribute("aria-label", t(s.run === "running" ? "studyPause" : "studyResume"));
}

function render(s: PanelState): void {
  if (s.lang !== lang) {
    lang = s.lang;
    setLang(s.lang);
    applyStatic();
    close.setAttribute("aria-label", t("miniHide"));
    close.title = t("miniHide");
    $("mini-read").title = t("miniOpen");
  }
  document.documentElement.dataset.noise = s.color;
  applyAccent(s.appearance.accent === "system" ? s.systemAccent : null);
  if (s.study) renderStudy(s.study);
}

toggle.addEventListener("click", () => void api.toggle());
close.addEventListener("click", () => void api.dismiss());

// Dragging anywhere but the buttons moves it; a click on the time opens the
// panel. Windows' own drag starts once the pointer has really moved.
let press: { x: number; y: number } | null = null;
root.addEventListener("pointerdown", (e) => {
  if (e.button !== 0 || (e.target as Element).closest("button")) return;
  press = { x: e.screenX, y: e.screenY };
});
root.addEventListener("pointermove", (e) => {
  if (!press) return;
  if (Math.hypot(e.screenX - press.x, e.screenY - press.y) > 3) {
    press = null;
    void api.drag();
  }
});
root.addEventListener("pointerup", (e) => {
  if (press && (e.target as Element).closest("#mini-read")) void api.openPanel();
  press = null;
});

api.onState(render);
api.onTick(renderStudy);
api.onShown(() => document.body.classList.replace("is-closing", "is-open"));
api.onHiding(() => document.body.classList.replace("is-open", "is-closing"));
if (isTauri) document.addEventListener("contextmenu", (e) => e.preventDefault());

const initial = await api.getState();
if (initial) render(initial);
document.body.classList.add("is-open");
void api.ready();
