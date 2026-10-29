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
}

export interface Appearance {
  theme: "system" | "light" | "dark";
  accent: "noise" | "system";
  mica: boolean;
  spectrum: boolean;
  reduceMotion: boolean;
  language: "auto" | "es" | "en";
}

export interface Backend {
  getState(): Promise<PanelState | null>;
  toggle(): Promise<void>;
  setColor(color: Color): Promise<void>;
  setVolume(volume: number): Promise<void>;
  setTimer(minutes: number): Promise<void>;
  hide(): Promise<void>;
  quit(): Promise<void>;
  openSettings(): Promise<void>;
  /** The page has rendered its first state. */
  ready(): Promise<void>;
  /** The first frame after being shown is on screen (timing). */
  painted(): Promise<void>;
  onState(cb: (s: PanelState) => void): void;
  /** 64 band levels in dB (power density), ~30 per second while visible. */
  onSpectrum(cb: (bands: number[]) => void): void;
  onTimerTick(cb: (remaining: number | null) => void): void;
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
    hide: () => call("hide_panel"),
    quit: () => call("quit"),
    openSettings: () => call("open_settings"),
    ready: () => call("panel_ready"),
    painted: () => call("panel_painted"),
    onState: (cb) => void listen<PanelState>("state-changed", (e) => cb(e.payload)),
    onSpectrum: (cb) => void listen<number[]>("spectrum", (e) => cb(e.payload)),
    onTimerTick: (cb) =>
      void listen<{ remaining: number | null }>("timer-tick", (e) => cb(e.payload.remaining)),
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
      reduceMotion: false,
      language: "auto",
    },
    systemAccent: params.get("accent") === "system" ? "#0078d4" : null,
  };
  let deadline = 0;
  const arm = () => {
    deadline = state.playing && state.timerMinutes ? Date.now() + state.timerMinutes * 60_000 : 0;
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
    state.timerRemaining = deadline ? Math.max(0, Math.ceil((deadline - Date.now()) / 1000)) : null;
    emit();
  }, 1000);
  return {
    getState: async () => ({ ...state }),
    toggle: async () => {
      state.playing = !state.playing;
      state.pausedFor = null;
      arm();
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
    hide: async () => {},
    quit: async () => {},
    openSettings: async () => {},
    ready: async () => {},
    painted: async () => {},
    onState: (cb) => listeners.state.push(cb),
    onSpectrum: (cb) => listeners.spectrum.push(cb),
    onTimerTick: () => {},
    onShown: (cb) => queueMicrotask(cb),
    onHiding: () => {},
  };
}

export const backend: Promise<Backend> = isTauri ? tauriBackend() : Promise.resolve(mockBackend());
