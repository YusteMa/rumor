# Manual QA checklist

Run before every release (SPEC §6). Tick each box on the build you are about to
publish. Sections are added as phases land.

## Audio engine (phase 1)

Until the tray exists (phase 2), use the console test bench:

```
cd src-tauri
cargo run --release --example engine
```

Type `help` for the commands. Start at a low volume (`v 20`).

### Sound
- [ ] Pink, brown and white each sound clean: no crackle, no "bubbling", no audible loop.
- [ ] `c pink` → `c brown` → `c white` while playing: smooth crossfade, no click.
- [ ] Switching colours keeps the same perceived loudness.
- [ ] `p` to start and stop: soft fade in and out, never a click.
- [ ] Volume jumps (`v 5` → `v 100` → `v 0`): smooth, no zipper noise.
- [ ] `f 3000` then play/pause: the fade takes ~3 s; `f 200`: ~0.2 s.
- [ ] Stereo feels wide (decorrelated), not centred in the head.

### Output changes (SPEC §3.3) — with each policy
Default policy is `policy pause`.

| Case | `pause` (default) | `always` | `keep` |
|---|---|---|---|
| Unplug wired headphones while playing | [ ] pauses, nothing from the speakers | [ ] pauses | [ ] continues on the speakers |
| Bluetooth headphones off / out of range | [ ] pauses, nothing from the speakers | [ ] pauses | [ ] continues on the speakers |
| Switch between two headphones | [ ] keeps playing | [ ] pauses | [ ] keeps playing |
| Change the default output in Windows (speakers → HDMI) | [ ] keeps playing | [ ] pauses | [ ] keeps playing |
| Unplug the only output device | [ ] pauses | [ ] pauses | [ ] pauses |

- [ ] After a pause caused by unplugging, `p` plays on the new output straight away.
- [ ] `resume on`, unplug headphones (pauses), plug them back: playback resumes.
- [ ] `resume off` (default): plugging back does not resume.
- [ ] Known limit to check: on laptops where the jack and the speakers are **one** Windows endpoint (some Realtek setups), unplugging does not change the endpoint; note what happens.

### Power
- [ ] Playing → sleep the PC → wake: stays paused; `p` plays normally.
- [ ] Same with hibernate.
- [ ] Paused for a while: `powercfg /requests` shows no audio stream from Rumor (the PC can sleep).

### Measurements (SPEC §3.4)
- [ ] `cargo run --release --example engine -- --check 20`: frames ≈ 100 % of expected, CPU < 1 % of the machine.
