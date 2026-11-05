// The Settings window's link to Rust. Outside Tauri a mock stands in, for
// design work in a browser.

import { isTauri, type Appearance, type Color, type Lang, type StudyMode, type Tag, type UpdateView } from "./api";

export type Action = "toggle" | "volumeUp" | "volumeDown" | "nextColor" | "openPanel" | "study";
export type Alerts = "bell" | "notification" | "both" | "none";
export type BreakNoise = "half" | "pause" | "keep";

export interface Settings {
  general: { alerts: Alerts; earBreakHours: number };
  study: {
    enabled: boolean;
    linkNoise: boolean;
    workMin: number;
    shortBreakMin: number;
    longBreakMin: number;
    cycles: number;
    breakNoise: BreakNoise;
    /** 0 = off. */
    idleMinutes: number;
  };
  sound: { startColor: "last" | Color; fadeMs: number; maxVolume: number; perOutputVolume: boolean; mono: boolean };
  playback: {
    onOutputChange: "pauseOnSpeakers" | "pauseAlways" | "keepPlaying";
    resumeOnReconnect: boolean;
    onLock: "pauseResumeWithHeadphones" | "pause" | "nothing";
    onLaunch: "silent" | "play" | "resume";
  };
  timer: { presets: [number, number, number]; endFadeS: number };
  shortcuts: Record<Action, string | null>;
  tray: { leftClick: "toggle" | "panel" };
  appearance: Appearance;
  startup: { launchAtLogin: boolean; checkUpdates: boolean };
  advanced: { outputDevice: string | null; buffer: "auto" | "ms20" | "ms50" | "ms100" };
  last: {
    color: Color;
    volume: number;
    timerMinutes: number;
    wasPlaying: boolean;
    outputVolumes: Record<string, number>;
    studyMode: StudyMode;
    studyTag: number | null;
  };
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
  tags: Tag[];
  /** A newer version, once a check found one. */
  update: UpdateView | null;
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
  /** Creates (`id` null) or renames / recolours a tag; rejects with a message. */
  upsertTag(id: number | null, name: string, color: string): Promise<Tag>;
  deleteTag(id: number): Promise<void>;
  /** The newer version, or null when this one is the latest; rejects offline. */
  checkUpdate(): Promise<string | null>;
  /** Rumor closes and reopens on success; rejects with a message. */
  installUpdate(): Promise<void>;
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
    upsertTag: (id, name, color) => invoke("tags_upsert", { id, name, color }),
    deleteTag: (id) => invoke("tags_delete", { id }),
    checkUpdate: () => invoke("update_check"),
    installUpdate: () => invoke("update_install"),
    onChanged: (cb) => void listen<SettingsView>("settings-changed", (e) => cb(e.payload)),
  };
}

function defaults(): Settings {
  return {
    general: { alerts: "both", earBreakHours: 3 },
    study: {
      enabled: false,
      linkNoise: true,
      workMin: 25,
      shortBreakMin: 5,
      longBreakMin: 15,
      cycles: 4,
      breakNoise: "half",
      idleMinutes: 0,
    },
    sound: { startColor: "last", fadeMs: 400, maxVolume: 1, perOutputVolume: true, mono: false },
    playback: { onOutputChange: "pauseOnSpeakers", resumeOnReconnect: false, onLock: "pauseResumeWithHeadphones", onLaunch: "silent" },
    timer: { presets: [25, 50, 90], endFadeS: 10 },
    shortcuts: { toggle: "Ctrl+Alt+P", volumeUp: null, volumeDown: null, nextColor: null, openPanel: null, study: "Ctrl+Alt+T" },
    tray: { leftClick: "toggle" },
    appearance: { theme: "system", accent: "noise", mica: true, spectrum: true, language: "auto" },
    startup: { launchAtLogin: false, checkUpdates: true },
    advanced: { outputDevice: null, buffer: "auto" },
    last: {
      color: "pink",
      volume: 0.4,
      timerMinutes: 0,
      wasPlaying: false,
      outputVolumes: {},
      studyMode: "stopwatch",
      studyTag: null,
    },
  };
}

/** Browser-only stand-in, for design work. */
function mockBackend(): SettingsBackend {
  const params = new URLSearchParams(location.search);
  let settings = defaults();
  // ?study=1 starts with "Modo estudio" on and a few tags.
  if (params.get("study")) settings.study.enabled = true;
  let tags: Tag[] = params.get("study")
    ? [
        { id: 1, name: "Cálculo", color: "sky" },
        { id: 2, name: "EDA", color: "sage" },
      ]
    : [];
  let nextTag = 3;
  // ?update=1 finds a newer version on "Buscar ahora".
  let update: UpdateView | null = null;
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
    version: "0.1.0-beta.1",
    systemAccent: "#0078d4",
    mica: false,
    tags: structuredClone(tags),
    update,
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
    upsertTag: async (id, name, color) => {
      const clean = name.trim();
      if (!clean) throw "a tag needs a name";
      if (tags.some((g) => g.id !== id && g.name.toLowerCase() === clean.toLowerCase())) {
        throw `there is already a tag called "${clean}"`;
      }
      const tag = { id: id ?? nextTag++, name: clean, color };
      tags = [...tags.filter((g) => g.id !== tag.id), tag].sort((a, b) => a.name.localeCompare(b.name));
      listeners.forEach((cb) => cb(view()));
      return tag;
    },
    deleteTag: async (id) => {
      tags = tags.filter((g) => g.id !== id);
      listeners.forEach((cb) => cb(view()));
    },
    checkUpdate: async () => {
      await new Promise((r) => setTimeout(r, 800));
      if (!params.get("update")) return null;
      update = { version: "0.1.0-beta.2", phase: "available" };
      listeners.forEach((cb) => cb(view()));
      return update.version;
    },
    installUpdate: async () => {
      if (update) update = { ...update, phase: "installing" };
      listeners.forEach((cb) => cb(view()));
    },
    onChanged: (cb) => listeners.push(cb),
  };
}

export const settingsBackend: Promise<SettingsBackend> = isTauri
  ? tauriBackend()
  : Promise.resolve(mockBackend());
