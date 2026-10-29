//! Audio engine. `noise` and `smoothing` are pure DSP with no `cpal` or Tauri
//! dependency, so they can be tested by rendering to buffers (phase 1).

mod analysis;
mod engine;
mod noise;
mod params;
mod smoothing;
