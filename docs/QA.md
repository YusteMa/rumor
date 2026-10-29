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

## Tray and shortcut (phase 2)

Run `npm run tauri dev` (or the installed build). No window should open.

- [ ] The icon appears in the tray in under a second; idle = short level bars, dimmed.
- [ ] Left click: noise fades in and the icon becomes the "waves"; left click again: fades out, idle icon.
- [ ] Tooltip while playing: `Rumor · Rosa · 40 %` (English: `Rumor · Pink · 40%`); paused: `Rumor · En pausa`.
- [ ] Unplug / switch off headphones while playing: idle icon, tooltip `Rumor · En pausa: se desconectaron los cascos`.
- [ ] Right click: menu with "Pausar/Reproducir  Ctrl+Alt+P" and "Salir de Rumor"; the first item follows the state.
- [ ] `Ctrl+Alt+P` toggles from any app, including a full-screen one.
- [ ] Spanish keyboard: typing with AltGr in an editor (`AltGr+E` = €, `AltGr+2` = @) still works and never toggles Rumor.
- [ ] Shortcut taken: register `Ctrl+Alt+P` in another app first (e.g. an AutoHotkey script `^!p::return`), start Rumor → notification says it uses `Ctrl+Alt+N`, and the menu shows it.
- [ ] Launch Rumor a second time: no second icon; a notification says it's already in the tray.
- [ ] "Salir de Rumor" while playing: the noise fades out, then the icon disappears. No click.
- [ ] Switch Windows between light and dark mode: the icon ink follows within a second.
- [ ] Display scaling 100 / 150 / 200 %: the icon is crisp (restart Rumor after changing it).
- [ ] Task Manager: Rumor with no panel uses ~20 MB of RAM and ~0 % CPU while paused.

## Measurements (SPEC §3.4)
- [ ] `cargo run --release --example engine -- --check 20`: frames ≈ 100 % of expected, CPU < 1 % of the machine.
