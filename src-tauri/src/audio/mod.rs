//! Audio engine. `noise`, `bell` and `smoothing` are pure DSP with no
//! `cpal` or Tauri dependency, so they can be tested by rendering to buffers.

pub mod analysis;
pub mod bell;
pub mod device_watch;
pub mod engine;
pub mod noise;
pub mod params;
pub mod smoothing;
