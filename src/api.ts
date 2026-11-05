// The panel's only link to Rust: commands out, events in (CLAUDE.md rule 3).
// Outside Tauri (a plain browser, for design work) a mock backend stands in.

export type Color = "pink" | "brown" | "white";
export type PauseReason =
  | "headphonesDisconnected"
  | "outputChanged"
  | "noOutput"
  | "suspended"
  | "locked";
export type Lang = "es" | "en";
export type StudyMode = "stopwatch" | "pomodoro";
export type StudyRun = "stopped" | "running" | "paused";
export type StudyPhase = "work" | "shortBreak" | "longBreak";

export interface Tag {
  id: number;
  name: string;
  /** A `--tag-*` token name. */
  color: string;
}

/** The study card (SPEC §4.3b); absent with "Modo estudio" off. */
export interface StudyView {
  run: StudyRun;
  mode: StudyMode;
  phase: StudyPhase;
  cycle: number;
  cycles: number;
  workedSecs: number;
  phaseLeftSecs: number | null;
  phaseSecs: number | null;
  todaySecs: number;
  weekSecs: number;
  tag: number | null;
  tags: Tag[];
  /** Minutes away while "¿Seguías estudiando?" waits. */
  idleMinutes: number | null;
}

export interface PanelState {
  playing: boolean;
  color: Color;
  /** Slider position in 0..1. */
  volume: number;
  timerMinutes: number;
  timerRemaining: number | null;
  pausedFor: PauseReason | null;
  output: { name: string; kind: "personal" | "public" } | null;
  shortcut: string | null;
  shortcutTaken: string | null;
  lang: Lang;
  mica: boolean;
  timerPresets: [number, number, number];
  appearance: Appearance;
  systemAccent: string | null;
  study: StudyView | null;
  /** A newer version, once a check found one. */
  update: UpdateView | null;
}

export interface UpdateView {
  version: string;
  phase: "available" | "installing" | "failed";
}

export interface Appearance {
  theme: "system" | "light" | "dark";
  accent: "noise" | "system";
  mica: boolean;
  spectrum: boolean;
  language: "auto" | "es" | "en";
}

export interface Backend {
  getState(): Promise<PanelState | null>;
  toggle(): Promise<void>;
  setColor(color: Color): Promise<void>;
  setVolume(volume: number): Promise<void>;
  setTimer(minutes: number): Promise<void>;
  studyToggle(): Promise<void>;
  studyFinish(): Promise<void>;
  studySetMode(mode: StudyMode): Promise<void>;
  studySetTag(tag: number | null): Promise<void>;
  studyResolveIdle(keep: boolean): Promise<void>;
  hide(): Promise<void>;
  quit(): Promise<void>;
  openSettings(): Promise<void>;
  /** Downloads and installs the update; Rumor reopens on its own. */
  installUpdate(): Promise<void>;
  /** The page has rendered its first state. */
  ready(): Promise<void>;
  /** The first frame after being shown is on screen (timing). */
  painted(): Promise<void>;
  onState(cb: (s: PanelState) => void): void;
  /** 64 band levels in dB (power density), ~30 per second while visible. */
  onSpectrum(cb: (bands: number[]) => void): void;
  onTimerTick(cb: (remaining: number | null) => void): void;
  /** Once a second while visible, with "Modo estudio" on. */
  onStudyTick(cb: (study: StudyView) => void): void;
  onShown(cb: () => void): void;
  onHiding(cb: () => void): void;
}

export const BANDS = 64;
export const F_MIN = 20;
export const F_MAX = 20_000;

/** Centre frequency of each band, as in `audio/analysis.rs`. */
export const bandCentres = Array.from(
  { length: BANDS },
  (_, i) => F_MIN * (F_MAX / F_MIN) ** ((i + 0.5) / BANDS),
);

export const isTauri = "__TAURI_INTERNALS__" in window;

async function tauriBackend(): Promise<Backend> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  const call = (cmd: string, args?: Record<string, unknown>) =>
    invoke<void>(cmd, args).catch((e) => console.error(cmd, e));
  return {
    getState: () => invoke<PanelState | null>("get_state"),
    toggle: () => call("toggle"),
    setColor: (color) => call("set_color", { color }),
    setVolume: (volume) => call("set_volume", { volume }),
    setTimer: (minutes) => call("set_timer", { minutes }),
    studyToggle: () => call("study_toggle"),
    studyFinish: () => call("study_finish"),
    studySetMode: (mode) => call("study_set_mode", { mode }),
    studySetTag: (tag) => call("study_set_tag", { tag }),
    studyResolveIdle: (keep) => call("study_resolve_idle", { keep }),
    hide: () => call("hide_panel"),
    quit: () => call("quit"),
    installUpdate: () => call("update_install"),
    openSettings: () => call("open_settings"),
    ready: () => call("panel_ready"),
    painted: () => call("panel_painted"),
    onState: (cb) => void listen<PanelState>("state-changed", (e) => cb(e.payload)),
    onSpectrum: (cb) => void listen<number[]>("spectrum", (e) => cb(e.payload)),
    onTimerTick: (cb) =>
      void listen<{ remaining: number | null }>("timer-tick", (e) => cb(e.payload.remaining)),
    onStudyTick: (cb) => void listen<StudyView>("study-tick", (e) => cb(e.payload)),
    onShown: (cb) => void listen("panel-shown", () => cb()),
    onHiding: (cb) => void listen("panel-hiding", () => cb()),
  };
}

/** Browser-only stand-in with a plausible spectrum, for design work. */
function mockBackend(): Backend {
  const params = new URLSearchParams(location.search);
  const state: PanelState = {
    playing: params.get("playing") === "1",
    color: (params.get("color") as Color) ?? "pink",
    volume: 0.4,
    timerMinutes: Number(params.get("timer") ?? 0),
    timerRemaining: null,
    pausedFor: (params.get("paused") as PauseReason) ?? null,
    output: params.get("nooutput") ? null : { name: "Auriculares", kind: "personal" },
    shortcut: "Ctrl+Alt+P",
    shortcutTaken: params.get("taken") ? "Ctrl+Alt+P" : null,
    lang: (params.get("lang") as Lang) ?? (navigator.language.startsWith("es") ? "es" : "en"),
    mica: false,
    timerPresets: [25, 50, 90],
    appearance: {
      theme: "system",
      accent: params.get("accent") === "system" ? "system" : "noise",
      mica: true,
      spectrum: params.get("spectrum") !== "0",
      language: "auto",
    },
    systemAccent: params.get("accent") === "system" ? "#0078d4" : null,
    study: null,
    update: params.get("update")
      ? { version: "0.1.0-beta.2", phase: params.get("update") as UpdateView["phase"] }
      : null,
  };
  // ?study=1 shows the card; ?study=pomodoro, ?idle=14 and ?tags=1 vary it.
  const study: StudyView | null = params.get("study")
    ? {
        run: "stopped",
        mode: params.get("study") === "pomodoro" ? "pomodoro" : "stopwatch",
        phase: "work",
        cycle: 1,
        cycles: 4,
        workedSecs: 0,
        phaseLeftSecs: null,
        phaseSecs: null,
        todaySecs: 3 * 3600 + 10 * 60,
        weekSecs: 12 * 3600 + 40 * 60,
        tag: null,
        tags: params.get("tags")
          ? [
              { id: 1, name: "Cálculo", color: "sky" },
              { id: 2, name: "EDA", color: "sage" },
              { id: 3, name: "Física", color: "amber" },
            ]
          : [],
        idleMinutes: params.get("idle") ? Number(params.get("idle")) : null,
      }
    : null;
  state.study = study;
  let workSince = 0;
  let workedBefore = 0;
  const POMODORO = [25 * 60, 5 * 60];
  const studyTick = () => {
    if (!study) return;
    const now = Date.now();
    const running = study.run === "running" && study.phase === "work";
    study.workedSecs = workedBefore + (running ? Math.floor((now - workSince) / 1000) : 0);
    if (study.mode === "pomodoro" && study.run !== "stopped") {
      const total = POMODORO[study.phase === "work" ? 0 : 1];
      study.phaseSecs = total;
      study.phaseLeftSecs = Math.max(0, total - (study.workedSecs % total));
    } else {
      study.phaseSecs = study.phaseLeftSecs = null;
    }
  };
  // Same rule as timer.rs: choosing restarts, pausing freezes, playing continues.
  let deadline = 0;
  let frozen = 0;
  const arm = () => {
    frozen = 0;
    deadline = state.playing && state.timerMinutes ? Date.now() + state.timerMinutes * 60_000 : 0;
  };
  const onPlaying = () => {
    if (state.playing && state.timerMinutes && !deadline) {
      deadline = Date.now() + (frozen || state.timerMinutes * 60_000);
      frozen = 0;
    } else if (!state.playing && deadline) {
      frozen = Math.max(0, deadline - Date.now());
      deadline = 0;
    }
  };
  arm();
  const listeners = { state: [] as ((s: PanelState) => void)[], spectrum: [] as ((b: number[]) => void)[] };
  const emit = () => listeners.state.forEach((cb) => cb({ ...state }));
  const slopes: Record<Color, number> = { pink: -3, brown: -6, white: 0 };
  setInterval(() => {
    if (!state.playing) return;
    const gain = 40 * Math.log10(Math.max(state.volume, 1e-3));
    const bands = bandCentres.map(
      (f) => -70 + gain + slopes[state.color] * Math.log2(f / 1000) + (Math.random() - 0.5) * 2,
    );
    listeners.spectrum.forEach((cb) => cb(bands));
  }, 33);
  setInterval(() => {
    const left = deadline ? deadline - Date.now() : frozen;
    state.timerRemaining = deadline || frozen ? Math.max(0, Math.ceil(left / 1000)) : null;
    studyTick();
    emit();
  }, 1000);
  // Linked, like the default: noise and stopwatch play and pause together.
  const studyRun = (run: StudyRun) => {
    if (!study || study.run === run) return;
    if (run === "running") workSince = Date.now();
    else {
      studyTick();
      workedBefore = run === "stopped" ? 0 : study.workedSecs;
      if (run === "stopped") study.todaySecs += study.workedSecs;
    }
    study.run = run;
    studyTick();
  };
  return {
    getState: async () => ({ ...state }),
    toggle: async () => {
      state.playing = !state.playing;
      state.pausedFor = null;
      onPlaying();
      studyRun(state.playing ? "running" : "paused");
      emit();
    },
    setColor: async (c) => {
      state.color = c;
      emit();
    },
    setVolume: async (v) => {
      state.volume = v;
      emit();
    },
    setTimer: async (m) => {
      state.timerMinutes = m;
      arm();
      emit();
    },
    studyToggle: async () => {
      if (!study) return;
      const run = study.run === "running" ? "paused" : "running";
      studyRun(run);
      state.playing = run === "running";
      onPlaying();
      emit();
    },
    studyFinish: async () => {
      studyRun("stopped");
      state.playing = false;
      onPlaying();
      emit();
    },
    studySetMode: async (mode) => {
      if (study && study.run === "stopped") study.mode = mode;
      studyTick();
      emit();
    },
    studySetTag: async (tag) => {
      if (study) study.tag = tag;
      emit();
    },
    studyResolveIdle: async (keep) => {
      if (study && study.idleMinutes && !keep) workedBefore = Math.max(0, workedBefore - study.idleMinutes * 60);
      if (study) study.idleMinutes = null;
      studyTick();
      emit();
    },
    hide: async () => {},
    quit: async () => {},
    installUpdate: async () => {
      if (state.update) state.update = { ...state.update, phase: "installing" };
      emit();
    },
    openSettings: async () => {},
    ready: async () => {},
    painted: async () => {},
    onState: (cb) => listeners.state.push(cb),
    onSpectrum: (cb) => listeners.spectrum.push(cb),
    onTimerTick: () => {},
    onStudyTick: () => {},
    onShown: (cb) => queueMicrotask(cb),
    onHiding: () => {},
  };
}

export const backend: Promise<Backend> = isTauri ? tauriBackend() : Promise.resolve(mockBackend());
