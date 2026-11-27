// Fase W0 (ROADMAP): ¿sigue sonando Rumor con el móvil bloqueado?
//
// Página de prueba, de usar y tirar. Hace sonar el motor de verdad de Rumor
// (WebAssembly, el mismo AudioWorklet que la demo web) por una de tres
// variantes, y mide qué parte del tiempo ha sonado de verdad: el reloj del
// AudioContext (`ac.currentTime`) solo avanza mientras el navegador genera
// audio, así que al compararlo con la hora real (`Date.now()`) se distingue
// «30 s de 30 s» de «0 s de 30 s: iOS lo congeló». El registro se guarda en
// localStorage, así que sobrevive aunque el sistema mate la página.
//
// Se compila con `npm run build:spike` (web/README.md); nunca forma parte de la app.

import engineUrl from "../../web/target/wasm32-unknown-unknown/release/rumor_web.wasm?url";
import { WORKLET } from "../web-engine";
import "../styles/tokens.css";
import "./spike.css";

type Variant = 1 | 2 | 3;

const LOG_KEY = "rumor-spike-log";
const VARIANT_KEY = "rumor-spike-variant";
/** Si entre dos comprobaciones pasa más que esto, la página estuvo congelada. */
const GAP_S = 3;

const byId = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const playButton = byId<HTMLButtonElement>("play");
const logList = byId<HTMLOListElement>("log");
const statusList = byId<HTMLDListElement>("status");

// ---------- Registro (sobrevive aunque el sistema mate la página) ----------

const lines: string[] = readLog();

function readLog(): string[] {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(LOG_KEY) ?? "[]");
    return Array.isArray(saved) ? saved.map(String) : [];
  } catch {
    return [];
  }
}

function log(message: string) {
  const time = new Date().toLocaleTimeString("es-ES", { hour12: false });
  const line = `${time}  ${message}`;
  lines.push(line);
  try {
    localStorage.setItem(LOG_KEY, JSON.stringify(lines.slice(-500)));
  } catch {
    // Modo privado o almacenamiento lleno: el registro no sobrevivirá a una recarga.
  }
  showLine(line);
}

function showLine(line: string) {
  const item = document.createElement("li");
  item.textContent = line;
  logList.append(item);
}

lines.forEach(showLine);

byId("copy").addEventListener("click", () => {
  void navigator.clipboard
    .writeText(lines.join("\n"))
    .then(() => log("Registro copiado"))
    .catch((e) => log(`No se pudo copiar: ${e}`));
});

byId("clear").addEventListener("click", () => {
  lines.length = 0;
  logList.replaceChildren();
  try {
    localStorage.removeItem(LOG_KEY);
  } catch {
    // No había nada guardado.
  }
});

// ---------- Variante (al cambiarla se recarga, para empezar cada prueba de cero) ----------

const variant = readVariant();

function readVariant(): Variant {
  try {
    const v = Number(localStorage.getItem(VARIANT_KEY));
    if (v === 2 || v === 3) return v;
  } catch {
    // Se queda la de abajo.
  }
  return 1;
}

document.querySelectorAll<HTMLInputElement>('input[name="variant"]').forEach((input) => {
  input.checked = Number(input.value) === variant;
  input.addEventListener("change", () => {
    try {
      localStorage.setItem(VARIANT_KEY, input.value);
    } catch {
      // Volverá a la variante 1.
    }
    location.reload();
  });
});

// ---------- Lo que cambia entre variantes ----------

/**
 * Por dónde sale el sonido del motor.
 * Variante 1: directo a los altavoces (`ac.destination`), como la demo web.
 * Variantes 2 y 3: a través de un elemento `<audio>`, que iOS trata como un
 * reproductor de música (controles en la pantalla de bloqueo, pausa al
 * quitar los cascos).
 * Devuelve ese elemento, o null en la variante 1.
 */
function connectOutput(ac: AudioContext, node: AudioWorkletNode, v: Variant): HTMLAudioElement | null {
  // Variante 1: el motor va directo a la salida de audio, como en la demo.
  if (v === 1) {
    node.connect(ac.destination);
    return null;
  }
  // Variantes 2 y 3: en vez de a la salida, el motor va a un «stream» (un
  // flujo de audio en directo, como el de una videollamada)...
  const stream = ac.createMediaStreamDestination();
  node.connect(stream);
  // ...y un elemento <audio> reproduce ese flujo. Para iOS, esto ya es «un
  // reproductor de música», no un sonido de una web. `new Audio()` crea el
  // elemento sin ponerlo en la página: no hace falta que se vea.
  const el = new Audio();
  el.srcObject = stream.stream;
  // play() no se llama aquí, sino en el clic: iOS solo deja arrancar el
  // audio dentro de un toque del usuario.
  return el;
}

/** Lo que sale en la pantalla de bloqueo (Media Session API). */
function setupMediaSession() {
  // Algunos navegadores no la tienen: entonces no se hace nada.
  if (!("mediaSession" in navigator)) return;
  // Lo que sale en la pantalla de bloqueo: título, «artista» e icono.
  // `new URL(..., location.href)` convierte "icon.png" en una dirección
  // completa (https://.../icon.png), que es lo que pide el sistema.
  navigator.mediaSession.metadata = new MediaMetadata({
    title: "Rumor",
    artist: "Ruido rosa",
    artwork: [{ src: new URL("icon.png", location.href).href, sizes: "512x512", type: "image/png" }],
  });
  // A propósito, NO se registran órdenes propias para play y pausa
  // (`setActionHandler`). Si la página las registra, al quitarse los cascos
  // iOS le pregunta a la página en vez de pausar el <audio> por sí mismo, y
  // el ruido se oye por el altavoz mientras el motor hace su fundido de
  // salida (400 ms). Sin ellas, iOS pausa y reanuda el <audio> al instante,
  // como con la música, y la página se entera por los eventos del elemento
  // (ver `watchElement`) para parar o arrancar el motor.
}

/** Variante 3: le dice a Safari que esto es reproducción, como la música, no un efecto de sonido. */
function useAudioSession() {
  // `navigator.audioSession` solo existe en Safari 16.4 o posterior, y
  // TypeScript no lo conoce todavía. `as` es como un cast en C++: le decimos
  // «trátalo como un navigator que puede tener audioSession». El `?` indica
  // que puede no estar.
  const session = (navigator as Navigator & { audioSession?: { type: string } }).audioSession;
  // Si existe, «playback» = reproducción, como la música: sigue sonando con
  // el interruptor de silencio y, con suerte, con la pantalla bloqueada.
  if (session) session.type = "playback";
  log(session ? "audioSession = playback" : "Este navegador no tiene audioSession");
}

// ---------- Motor y medida ----------

let ac: AudioContext | null = null;
let node: AudioWorkletNode | null = null;
let audio: HTMLAudioElement | null = null;
let started = false;
let playing = false;
/** Última comprobación del reloj del audio frente a la hora real, y totales desde que sonó. */
let mark: { wall: number; audio: number } | null = null;
const total = { wall: 0, audio: 0 };
/** Lo mismo que `total`, pero se pone a cero cada minuto. */
const minute = { wall: 0, audio: 0 };

// Todo lo que se puede dejar listo antes del clic se hace ya, para que el clic
// solo tenga trabajo inmediato: iOS solo deja arrancar el audio dentro del clic.
async function prepare() {
  if (variant === 3) useAudioSession();
  const context = (ac = new AudioContext());
  // Cada vez que el sistema cambia el estado del audio (running, suspended,
  // interrupted...), se apunta y se actualiza la tabla.
  context.addEventListener("statechange", () => {
    log(`AudioContext: ${context.state}`);
    showStatus();
  });
  const bytes = await fetch(engineUrl).then((r) => {
    if (!r.ok) throw new Error(`engine: HTTP ${r.status}`);
    return r.arrayBuffer();
  });
  const url = URL.createObjectURL(new Blob([WORKLET], { type: "text/javascript" }));
  await context.audioWorklet.addModule(url);
  URL.revokeObjectURL(url);
  node = new AudioWorkletNode(context, "rumor", {
    outputChannelCount: [2],
    processorOptions: { bytes, seed: crypto.getRandomValues(new Uint32Array(1))[0] },
  });
  node.port.postMessage(["volume", 0.5]);
  playButton.disabled = false;
  playButton.textContent = "Sonar";
  log(`Lista · variante ${variant} · ${context.sampleRate} Hz · ${installed() ? "app instalada" : "navegador"}`);
}

/** El fundido de salida del motor: el mismo que `FADE_OUT_MS` en noise.rs. */
const ENGINE_FADE_OUT_MS = 400;

/**
 * Arranca o para el ruido.
 * `from` dice quién lo pide: "page" es el botón de la página; "system" es
 * iOS, que ya ha pausado o reanudado el <audio> por su cuenta (cascos,
 * llamada, pantalla de bloqueo), así que aquí solo se sigue al motor.
 * `from: "page" | "system" = "page"` es un parámetro con valor por defecto,
 * como en C++, y solo admite esos dos textos.
 */
function setPlaying(next: boolean, from: "page" | "system" = "page") {
  if (!ac || !node) return;
  if (!started) {
    started = true;
    audio = connectOutput(ac, node, variant);
    if (audio) watchElement(audio);
    if (variant !== 1) setupMediaSession();
  }
  // Al parar, se apunta el total de esa tanda, para no depender de mirar la tabla.
  if (playing && !next) {
    check();
    log(`Total hasta ahora: audio vivo ${pct(total.audio, total.wall)} de ${fmt(total.wall)}`);
  }
  playing = next;
  node.port.postMessage(["playing", playing]);
  if (playing) {
    void ac.resume();
    if (from === "page") audio?.play().catch((e) => log(`<audio> no arranca: ${e}`));
    mark ??= { wall: Date.now() / 1000, audio: ac.currentTime };
  } else if (from === "page" && audio) {
    // Con el botón de la página, primero se deja que el motor haga su
    // fundido de salida y después se pausa el <audio>, para que no se oiga
    // un corte. Así la pantalla de bloqueo también muestra «en pausa».
    const el = audio;
    setTimeout(() => {
      if (!playing) el.pause();
    }, ENGINE_FADE_OUT_MS + 50);
  }
  if ("mediaSession" in navigator) navigator.mediaSession.playbackState = playing ? "playing" : "paused";
  playButton.textContent = playing ? "Parar" : "Sonar";
  log(playing ? "▶ Suena" : "■ Parado");
}

playButton.addEventListener("click", () => setPlaying(!playing));

/** Apunta lo que el sistema le hace al elemento `<audio>`, y el motor lo sigue (regla 10). */
function watchElement(el: HTMLAudioElement) {
  for (const type of ["play", "playing", "pause", "waiting", "stalled", "suspend", "ended", "error"]) {
    el.addEventListener(type, () => log(`<audio>: ${type}`));
  }
  // Si el <audio> se pausa mientras el ruido debería sonar, no ha sido el
  // botón de la página: ha sido iOS (cascos fuera, llamada, pantalla de
  // bloqueo). iOS ya lo ha silenciado; aquí solo se para también el motor.
  el.addEventListener("pause", () => {
    if (playing) {
      log("El sistema ha pausado el <audio> → paro el motor");
      setPlaying(false, "system");
    }
  });
  // Y al revés: si el <audio> vuelve a sonar sin que se haya pulsado el
  // botón, ha sido iOS (cascos dentro, pantalla de bloqueo): se arranca el
  // motor, con su fundido de entrada.
  el.addEventListener("play", () => {
    if (!playing) {
      log("El sistema ha reanudado el <audio> → arranco el motor");
      setPlaying(true, "system");
    }
  });
}

/** Compara el reloj del audio con la hora real desde la última comprobación. */
function check(force = false) {
  if (!ac || !mark) return;
  const now = { wall: Date.now() / 1000, audio: ac.currentTime };
  const wall = now.wall - mark.wall;
  const ran = now.audio - mark.audio;
  mark = now;
  if (playing) {
    total.wall += wall;
    total.audio += ran;
    minute.wall += wall;
    minute.audio += ran;
  }
  if (playing && wall > GAP_S) {
    log(`Hueco de ${fmt(wall)}: el audio avanzó ${fmt(ran)} (${pct(ran, wall)})`);
  }
  // Cada minuto sonando (o al volver a la página) se apunta qué parte de ese
  // tiempo avanzó el audio. Así se ve aunque la página no se haya congelado:
  // en el iPhone el código siguió vivo, pero hay que saber si el audio también.
  if (playing && minute.wall > 0 && (force || minute.wall >= 60)) {
    log(`Último tramo: el audio avanzó ${fmt(minute.audio)} de ${fmt(minute.wall)} (${pct(minute.audio, minute.wall)})`);
    minute.wall = 0;
    minute.audio = 0;
  }
  showStatus();
}

function showStatus() {
  const rows: [string, string][] = [
    ["Variante", String(variant)],
    ["Modo", installed() ? "app instalada" : "navegador"],
    ["audioSession", "audioSession" in navigator ? "existe" : "no existe"],
    ["AudioContext", ac?.state ?? "—"],
    ["Audio vivo", total.wall > 0 ? `${pct(total.audio, total.wall)} de ${fmt(total.wall)}` : "—"],
  ];
  statusList.replaceChildren(
    ...rows.flatMap(([k, v]) => {
      const dt = document.createElement("dt");
      const dd = document.createElement("dd");
      dt.textContent = k;
      dd.textContent = v;
      return [dt, dd];
    }),
  );
}

function installed(): boolean {
  return matchMedia("(display-mode: standalone)").matches || (navigator as { standalone?: boolean }).standalone === true;
}

const fmt = (s: number) => (s < 90 ? `${s.toFixed(1)} s` : `${Math.floor(s / 60)} min ${Math.round(s % 60)} s`);
const pct = (part: number, whole: number) => `${Math.min(100, (100 * part) / whole).toFixed(1)} %`;

// ---------- Lo que el sistema le hace a la página ----------

setInterval(() => check(), 1000);
document.addEventListener("visibilitychange", () => {
  log(`Página ${document.visibilityState === "visible" ? "visible" : "oculta"}`);
  if (document.visibilityState === "visible") check(true);
});
addEventListener("pagehide", () => log("pagehide"));
addEventListener("pageshow", (e) => e.persisted && log("pageshow (vuelve de la caché)"));
document.addEventListener("freeze", () => log("freeze"));
document.addEventListener("resume", () => log("resume"));
navigator.mediaDevices?.addEventListener("devicechange", () => log("Ha cambiado la salida de audio"));

showStatus();
prepare().catch((e) => {
  playButton.textContent = "Error";
  log(`Error al preparar: ${e}`);
});
