//! Audio engine. `noise` and `smoothing` are pure DSP with no `cpal` or Tauri
//! dependency, so they can be tested by rendering to buffers (phase 1).

mod analysis;
mod engine;
pub mod noise;
pub mod params;
pub mod smoothing;
