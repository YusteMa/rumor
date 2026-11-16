// The panel on a web page (marioyuste.com): the browser stand-in backend
// plays through Rumor's own audio engine, compiled to WebAssembly
// (web/README.md). Only built with `npm run build:web`; the Tauri app never
// loads this file.

import engineUrl from "../web/target/wasm32-unknown-unknown/release/rumor_web.wasm?url";
import type { Color } from "./api";
import { webPage } from "./web-page";

export interface WebEngine {
  setPlaying(playing: boolean): void;
  setColor(color: Color): void;
  setVolume(volume: number): void;
  onBands(cb: (bands: number[]) => void): void;
}

const COLOR_INDEX: Record<Color, number> = { pink: 0, brown: 1, white: 2 };

// Runs on the browser's audio thread. It gets the .wasm bytes, compiles them
// there and asks the engine for each 128-frame block, like the cpal callback.
const WORKLET = `
class Rumor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const { bytes, seed } = options.processorOptions;
    this.m = new WebAssembly.Instance(new WebAssembly.Module(bytes), {}).exports;
    this.m.rumor_init(sampleRate, seed);
    this.max = this.m.rumor_max_frames();
    this.bands = this.m.rumor_bands_len();
    this.port.onmessage = ({ data: [key, value] }) => {
      if (key === "playing") this.m.rumor_set_playing(value ? 1 : 0);
      else if (key === "color") this.m.rumor_set_color(value);
      else if (key === "volume") this.m.rumor_set_volume(value);
    };
  }
  process(_inputs, [out]) {
    const n = out[0].length;
    const p = this.m.rumor_render(n);
    const mem = this.m.memory.buffer;
    out[0].set(new Float32Array(mem, p, n));
    if (out[1]) out[1].set(new Float32Array(mem, p + this.max * 4, n));
    const q = this.m.rumor_bands();
    if (q) this.port.postMessage(Array.from(new Float32Array(mem, q, this.bands)));
    return true;
  }
}
registerProcessor("rumor", Rumor);
`;

export function createWebEngine(): WebEngine {
  webPage();

  const bandListeners: ((bands: number[]) => void)[] = [];
  // Last value of each control, replayed once the engine is up.
  const pending: [string, unknown][] = [];
  let port: MessagePort | null = null;
  let context: AudioContext | null = null;

  const send = (key: string, value: unknown) => {
    if (port) port.postMessage([key, value]);
    else {
      const i = pending.findIndex(([k]) => k === key);
      if (i >= 0) pending.splice(i, 1);
      pending.push([key, value]);
    }
  };

  // Built on the first play: browsers only allow audio after a click, and the
  // click is what calls this.
  const start = async () => {
    const ac = (context = new AudioContext());
    const [bytes] = await Promise.all([
      fetch(engineUrl).then((r) => {
        if (!r.ok) throw new Error(`engine: HTTP ${r.status}`);
        return r.arrayBuffer();
      }),
      ac.resume(),
    ]);
    const url = URL.createObjectURL(new Blob([WORKLET], { type: "text/javascript" }));
    await ac.audioWorklet.addModule(url);
    URL.revokeObjectURL(url);
    const node = new AudioWorkletNode(ac, "rumor", {
      outputChannelCount: [2],
      processorOptions: { bytes, seed: crypto.getRandomValues(new Uint32Array(1))[0] },
    });
    node.connect(ac.destination);
    node.port.onmessage = ({ data }) => bandListeners.forEach((cb) => cb(data));
    port = node.port;
    pending.splice(0).forEach((m) => node.port.postMessage(m));
  };

  addEventListener("pagehide", () => void context?.close());

  return {
    setPlaying(playing) {
      if (playing && !context) start().catch((e) => console.error(e));
      else void context?.resume();
      send("playing", playing);
    },
    setColor: (color) => send("color", COLOR_INDEX[color]),
    setVolume: (volume) => send("volume", volume),
    onBands: (cb) => bandListeners.push(cb),
  };
}
