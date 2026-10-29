//! Console test bench for the audio engine (phase 1, before the tray exists).
//!
//!   cargo run --example engine                 interactive; type `help`
//!   cargo run --example engine -- --check 10   plays at volume 0 for 10 s and
//!                                              reports frames and CPU use

use std::io::BufRead;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait};
use rumor_lib::audio::device_watch::{OutputChangePolicy, OutputKind};
use rumor_lib::audio::engine::{AudioEngine, EngineConfig, EngineEvent};
use rumor_lib::audio::noise::{Controls, NoiseColor};

struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("  [{}] {}", r.level(), r.args());
        }
    }
    fn flush(&self) {}
}

const HELP: &str = "  p                 play / pause
  c pink|brown|white
  v 0-100           volume
  f 200-3000        fade length in ms
  policy pause|always|keep
  resume on|off     resume when headphones reconnect
  d                 list output devices
  i                 current output
  q                 quit";

fn main() {
    let _ = log::set_logger(&StderrLogger).map(|()| log::set_max_level(log::LevelFilter::Info));
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--check") {
        let seconds = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(10.0);
        check(seconds);
        return;
    }
    interactive();
}

fn print_event(event: EngineEvent) {
    match event {
        EngineEvent::AutoPaused(reason) => println!("» paused by the engine: {reason:?}"),
        EngineEvent::AutoResumed => println!("» resumed: headphones are back"),
        EngineEvent::OutputChanged(Some(o)) => println!(
            "» output: {} [{:?}] {} Hz, {} ch, {}",
            o.endpoint.name, o.endpoint.kind, o.sample_rate, o.channels, o.sample_format
        ),
        EngineEvent::OutputChanged(None) => println!("» no audio output"),
        EngineEvent::StreamError(e) => println!("» stream error: {e}"),
    }
}

fn interactive() {
    let mut config = EngineConfig::default();
    let mut controls = Controls {
        volume: 0.3,
        ..Controls::default()
    };
    let engine = match AudioEngine::start(controls, config, print_event) {
        Ok(e) => e,
        Err(e) => return eprintln!("{e}"),
    };
    println!("Rumor engine test bench. Starts paused at 30 % volume.\n{HELP}");

    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let mut words = line.split_whitespace();
        match (words.next(), words.next()) {
            (Some("p"), _) => println!("{}", if engine.toggle() { "playing" } else { "paused" }),
            (Some("c"), Some(c)) => match c {
                "pink" => engine.set_color(NoiseColor::Pink),
                "brown" => engine.set_color(NoiseColor::Brown),
                "white" => engine.set_color(NoiseColor::White),
                _ => println!("pink, brown or white"),
            },
            (Some("v"), Some(v)) => match v.parse::<f32>() {
                Ok(v) => {
                    controls.volume = (v / 100.0).clamp(0.0, 1.0);
                    engine.set_volume(controls.volume);
                }
                Err(_) => println!("v 0-100"),
            },
            (Some("f"), Some(ms)) => match ms.parse::<f32>() {
                Ok(ms) => {
                    let ms = ms.clamp(200.0, 3_000.0);
                    engine.set_fade_in_ms(ms);
                    engine.set_fade_out_ms(ms);
                }
                Err(_) => println!("f 200-3000"),
            },
            (Some("policy"), Some(p)) => {
                config.on_output_change = match p {
                    "pause" => OutputChangePolicy::PauseOnSpeakers,
                    "always" => OutputChangePolicy::PauseAlways,
                    "keep" => OutputChangePolicy::KeepPlaying,
                    _ => {
                        println!("pause, always or keep");
                        continue;
                    }
                };
                engine.set_config(config);
                println!("{:?}", config.on_output_change);
            }
            (Some("resume"), Some(v)) => {
                config.resume_on_reconnect = v == "on";
                engine.set_config(config);
                println!("resume on reconnect: {}", config.resume_on_reconnect);
            }
            (Some("d"), _) => list_devices(),
            (Some("i"), _) => println!(
                "{:?} (paused for: {:?})",
                engine.output(),
                engine.paused_for()
            ),
            (Some("q"), _) => break,
            (Some(_), _) => println!("{HELP}"),
            (None, _) => {}
        }
    }
}

fn list_devices() {
    let host = cpal::default_host();
    let default = host.default_output_device().and_then(|d| d.id().ok());
    let Ok(devices) = host.output_devices() else {
        return println!("could not list devices");
    };
    for d in devices {
        let id = d.id().ok();
        let (name, kind) = d.description().map_or_else(
            |_| (d.to_string(), String::from("?")),
            |desc| {
                let kind = OutputKind::from_device_type(desc.device_type());
                (
                    desc.name().to_string(),
                    format!("{:?} → {kind:?}", desc.device_type()),
                )
            },
        );
        let mark = if id.is_some() && id == default {
            "*"
        } else {
            " "
        };
        println!("{mark} {name}  [{kind}]");
    }
}

#[cfg(windows)]
fn cpu_time() -> Duration {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let (mut a, mut b, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    let ok =
        unsafe { GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut kernel, &mut user) }
            .is_ok();
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    if ok {
        Duration::from_nanos((ticks(kernel) + ticks(user)) * 100)
    } else {
        Duration::ZERO
    }
}

#[cfg(not(windows))]
fn cpu_time() -> Duration {
    Duration::ZERO
}

/// Plays at volume 0 (inaudible) and reports whether the callback keeps up
/// and how much CPU the engine costs.
fn check(seconds: f64) {
    let controls = Controls {
        playing: true,
        volume: 0.0,
        ..Controls::default()
    };
    let engine = match AudioEngine::start(controls, EngineConfig::default(), print_event) {
        Ok(e) => e,
        Err(e) => return eprintln!("{e}"),
    };
    // Let the stream open (and calibration run) before measuring.
    std::thread::sleep(Duration::from_secs(1));
    let Some(output) = engine.output() else {
        return println!("FAIL: no output opened");
    };
    let (f0, c0, t0) = (engine.frames_rendered(), cpu_time(), Instant::now());
    std::thread::sleep(Duration::from_secs_f64(seconds));
    let (frames, cpu, wall) = (engine.frames_rendered() - f0, cpu_time() - c0, t0.elapsed());

    let expected = f64::from(output.sample_rate) * wall.as_secs_f64();
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get()) as f64;
    let one_core = 100.0 * cpu.as_secs_f64() / wall.as_secs_f64();
    println!(
        "output:    {} [{:?}]",
        output.endpoint.name, output.endpoint.kind
    );
    println!(
        "format:    {} Hz, {} ch, {}",
        output.sample_rate, output.channels, output.sample_format
    );
    println!(
        "frames:    {frames} rendered, {expected:.0} expected ({:.2} %)",
        100.0 * frames as f64 / expected
    );
    println!(
        "cpu:       {one_core:.2} % of one core, {:.3} % of the machine ({cores} threads)",
        one_core / cores
    );
    engine.set_playing(false);
    std::thread::sleep(Duration::from_millis(600));
    println!(
        "after pause: playing={}, frames still advancing: {}",
        engine.is_playing(),
        {
            let a = engine.frames_rendered();
            std::thread::sleep(Duration::from_millis(300));
            engine.frames_rendered() != a
        }
    );
}
