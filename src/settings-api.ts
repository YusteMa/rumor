// The Settings window's link to Rust. Outside Tauri a mock stands in, for
// design work in a browser.

import { isTauri, type Appearance, type Color, type Lang } from "./api";

export type Action = "toggle" | "volumeUp" | "volumeDown" | "nextColor" | "openPanel";

export interface Settings {
  sound: { startColor: "last" | Color; fadeMs: number; maxVolume: number; perOutputVolume: boolean; mono: boolean };
  playback: {
    onOutputChange: "pauseOnSpeakers" | "pauseAlways" | "keepPlaying";
    resumeOnReconnect: boolean;
    onLock: "pauseResumeWithHeadphones" | "pause" | "nothing";
    onLaunch: "silent" | "play" | "resume";
  };
  timer: { presets: [number, number, number]; endFadeS: number; notifyOnEnd: boolean };
  shortcuts: Record<Action, string | null>;
  tray: { leftClick: "toggle" | "panel"; showRemaining: boolean };
  appearance: Appearance;
  startup: { launchAtLogin: boolean };
  advanced: { outputDevice: string | null; buffer: "auto" | "ms10" | "ms20" | "ms50" };
  last: { color: Color; volume: number; timerMinutes: number; wasPlaying: boolean; outputVolumes: Record<string, number> };
}

export interface Endpoint {
  id: string;
  name: string;
  kind: "personal" | "public";
}

export interface SettingsView {
  settings: Settings;
  shortcuts: { action: Action; active: string | null; unavailable: string | null }[];
  output: { endpoint: Endpoint; sampleRate: number; channels: number; sampleFormat: string } | null;
  lang: Lang;
  version: string;
  systemAccent: string | null;
  mica: boolean;
}

export type ShortcutError = { kind: "invalid" | "noModifier" | "typesCharacter" | "duplicate" | "taken" };

/** Nested partial, for `update`. */
export type Patch<T> = { [K in keyof T]?: T[K] extends object ? Patch<T[K]> : T[K] };

export interface SettingsBackend {
  get(): Promise<SettingsView | null>;
  ready(): Promise<void>;
  update(patch: Patch<Settings>): Promise<SettingsView | null>;
  /** Rejects with a `ShortcutError`. */
  setShortcut(action: Action, shortcut: string | null): Promise<SettingsView | null>;
  listOutputs(): Promise<Endpoint[]>;
  exportSettings(): Promise<boolean>;
  importSettings(): Promise<boolean>;
  reset(): Promise<SettingsView | null>;
  openLogs(): Promise<void>;
  onChanged(cb: (view: SettingsView) => void): void;
}

async function tauriBackend(): Promise<SettingsBackend> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  return {
    get: () => invoke("get_settings"),
    ready: () => invoke("settings_ready"),
    update: (patch) => invoke("update_settings", { patch }),
    setShortcut: (action, shortcut) => invoke("set_shortcut", { action, shortcut }),
    listOutputs: () => invoke("list_outputs"),
    exportSettings: () => invoke("export_settings"),
    importSettings: () => invoke("import_settings"),
    reset: () => invoke("reset_settings"),
    openLogs: () => invoke("open_logs"),
    onChanged: (cb) => void listen<SettingsView>("settings-changed", (e) => cb(e.payload)),
  };
}

function defaults(): Settings {
  return {
    sound: { startColor: "last", fadeMs: 400, maxVolume: 1, perOutputVolume: true, mono: false },
    playback: { onOutputChange: "pauseOnSpeakers", resumeOnReconnect: false, onLock: "pauseResumeWithHeadphones", onLaunch: "silent" },
    timer: { presets: [25, 50, 90], endFadeS: 10, notifyOnEnd: true },
    shortcuts: { toggle: "Ctrl+Alt+P", volumeUp: null, volumeDown: null, nextColor: null, openPanel: null },
    tray: { leftClick: "toggle", showRemaining: true },
    appearance: { theme: "system", accent: "noise", mica: true, spectrum: true, reduceMotion: false, language: "auto" },
    startup: { launchAtLogin: false },
    advanced: { outputDevice: null, buffer: "auto" },
    last: { color: "pink", volume: 0.4, timerMinutes: 0, wasPlaying: false, outputVolumes: {} },
  };
}

/** Browser-only stand-in, for design work. */
function mockBackend(): SettingsBackend {
  const params = new URLSearchParams(location.search);
  let settings = defaults();
  const listeners: ((v: SettingsView) => void)[] = [];
  const view = (): SettingsView => ({
    settings: structuredClone(settings),
    shortcuts: (Object.keys(settings.shortcuts) as Action[]).map((action) => ({
      action,
      active: settings.shortcuts[action],
      unavailable: null,
    })),
    output: {
      endpoint: { id: "wasapi:1", name: "Auriculares (WH-1000XM6)", kind: "personal" },
      sampleRate: 48000,
      channels: 2,
      sampleFormat: "f32",
    },
    lang: (params.get("lang") as Lang) ?? "es",
    version: "0.1.0",
    systemAccent: "#0078d4",
    mica: false,
  });
  const merge = (target: Record<string, unknown>, patch: Record<string, unknown>) => {
    for (const [k, v] of Object.entries(patch)) {
      if (v && typeof v === "object" && !Array.isArray(v)) merge(target[k] as Record<string, unknown>, v as Record<string, unknown>);
      else target[k] = v;
    }
  };
  return {
    get: async () => view(),
    ready: async () => {},
    update: async (patch) => {
      merge(settings as unknown as Record<string, unknown>, patch as Record<string, unknown>);
      return view();
    },
    setShortcut: async (action, shortcut) => {
      if (shortcut?.startsWith("Ctrl+Alt+E")) throw { kind: "typesCharacter" } satisfies ShortcutError;
      settings.shortcuts[action] = shortcut;
      return view();
    },
    listOutputs: async () => [
      { id: "wasapi:1", name: "Auriculares (WH-1000XM6)", kind: "personal" },
      { id: "wasapi:2", name: "Altavoces (Realtek(R) Audio)", kind: "public" },
    ],
    exportSettings: async () => true,
    importSettings: async () => false,
    reset: async () => {
      const last = settings.last;
      settings = { ...defaults(), last };
      return view();
    },
    openLogs: async () => {},
    onChanged: (cb) => listeners.push(cb),
  };
}

export const settingsBackend: Promise<SettingsBackend> = isTauri
  ? tauriBackend()
  : Promise.resolve(mockBackend());
