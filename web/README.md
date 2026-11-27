# Rumor web engine

Rumor's panel and audio engine (compiled to WebAssembly), for the demo on
[marioyuste.com](https://marioyuste.com/proyectos/rumor/).

`src/lib.rs` has no DSP of its own: it includes the app's own
`src-tauri/src/audio/{noise,smoothing,bell,analysis}.rs` and exposes a few C
functions for an AudioWorklet. Improve the sound in the app, rebuild, and the
demo gets it too.

## Build

One-time setup on any machine with Rust:

```sh
rustup target add wasm32-unknown-unknown
```

Then, from the repo root:

```sh
npm run build:web
```

It compiles the engine to WebAssembly and builds the panel alone, in Vite
mode `web`, into `web/dist/`. In that mode `src/api.ts` plays through
`src/web-engine.ts`; the Tauri app never includes it. On the web page the
panel is always dark, and the Windows-only parts (footer, Statistics, mini
clock) are hidden.

Copy `web/dist/` to the website as `public/rumor/panel/` and update the
version noted in `src/components/DemoRuido.astro`.

## Background-audio test (phase W0)

`npm run build:spike` builds the throwaway test page in `src/spike/` into
`web/dist-spike/`. It answers one question for the phone app (SPEC §10.3): does
Rumor keep sounding with the screen locked? How to test, and the results:
[`docs/web-spike.md`](../docs/web-spike.md). Deleted once W0 has its answer.

## API

| Function | What it does |
|---|---|
| `rumor_init(sample_rate, seed)` | Builds and calibrates the engine (~10 ms) |
| `rumor_set_playing(0/1)` | Starts or stops, with the app's fades |
| `rumor_set_color(0/1/2)` | Pink, brown, white (150 ms crossfade) |
| `rumor_set_volume(0..1)` | Slider position; the engine applies its curve |
| `rumor_render(frames)` | Renders up to `rumor_max_frames()`; returns a planar buffer (L, then R at `+max_frames`) |
| `rumor_bands()` | 64 spectrum bands in dB when a frame is due (~30 fps), else null |
