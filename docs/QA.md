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
- [ ] `Ctrl+Alt+P` toggles from any app, including a full-screen one.
- [ ] Spanish keyboard: typing with AltGr in an editor (`AltGr+E` = €, `AltGr+2` = @) still works and never toggles Rumor.
- [ ] Shortcut taken: register `Ctrl+Alt+P` in another app first (e.g. an AutoHotkey script `^!p::return`), start Rumor → notification says it uses `Ctrl+Alt+N`, and the panel footer says so.
- [ ] Launch Rumor a second time: no second icon; the panel opens.
- [ ] Switch Windows between light and dark mode: the icon ink follows within a second.
- [ ] Display scaling 100 / 150 / 200 %: the icon is crisp (restart Rumor after changing it).
- [ ] Task Manager: Rumor with no panel uses ~20 MB of RAM and ~0 % CPU while paused.

## Panel (phase 3)

Reference screenshots: `docs/screenshots/panel-light.png`, `panel-dark.png`, `accents.png`.

- [ ] Right click on the tray icon: the panel fades in, 8 px upward, in under 150 ms, next to the icon.
- [ ] Click anywhere outside: it fades out. Right click on the icon while open: it closes and stays closed.
- [ ] Real clicks, not a script: right click opens the panel and it stays open, with the hover first and with a fast click straight onto the icon. Ten right clicks in a row alternate open / closed every time. Alternating left and right clicks: left toggles the noise (closing the panel if open), right opens it. Also with the icon inside the tray overflow (^) flyout.
- [ ] Taskbar at the bottom, top, left and right (and auto-hidden): the panel sits next to it, 12 px from the edges, never off-screen.
- [ ] Two monitors with different scaling: the panel opens on the monitor with the tray, crisp and the right size.
- [ ] Windows 11: Mica behind the panel, rounded corners, a warm 1 px border in light and dark. Windows 10: solid colour, no glitches.
- [ ] Windows light and dark mode: the panel follows the system.
- [ ] Play: the icon morphs into pause, the halo breathes slowly (4 s), the status dot turns pink, the spectrum fades in.
- [ ] Pink / brown / white: the curve morphs to the new slope (−3 / −6 / 0 dB/oct) and lies on the dashed line.
- [ ] The accent follows the noise (dusty pink, toasted caramel, blue-grey pearl) on play, halo, curve, selection, slider and focus ring, crossfading in ~300 ms together with the curve. Text stays neutral; the tray icon stays monochrome.
- [ ] Panel lifecycle: hover the tray icon for half a second, then right click: the panel appears at once. Close it and wait 5 minutes: Task Manager shows Rumor back near 5 MB (WebView2 processes gone). Hovering again rebuilds it.
- [ ] Volume drag is smooth, the value follows the thumb, the tray tooltip follows the value.
- [ ] Timer 25 min while playing: "quedan 24:59" counts down here and in the tooltip; pausing cancels it; at the end the noise fades out over ~10 s.
- [ ] Unplug headphones while the panel is open: "En pausa: se desconectaron los cascos" under the title.
- [ ] No output device at all: the play button is disabled and the panel says there is no audio output.
- [ ] Keyboard only: Tab reaches every control with a visible pink focus ring; arrows move within Color and Timer; Space plays/pauses; Esc closes.
- [ ] Windows "Animation effects" off (reduced motion): no halo and no movement, only fades.
- [ ] Scaling 100 / 150 / 200 %: text and the spectrum are crisp, nothing is clipped.
- [ ] Footer: "Salir" fades the noise out and quits; "Ajustes" says it arrives in the next version.

## Measurements (SPEC §3.4)
- [ ] `cargo run --release --example engine -- --check 20`: frames ≈ 100 % of expected, CPU < 1 % of the machine.
- [ ] Memory (RAM = private working set incl. WebView2, bundled build), measured 2026-10-29: no panel ~3 MB; panel open ~154 MB; hidden and suspended ~148 MB; 5 minutes after closing (webview destroyed) ~6 MB. Executable: 6.0 MB.
- [ ] Panel open latency, click to first frame on screen (2026-10-29): already built 63–78 ms; hover ≥ 600 ms before the click ~100 ms; hover 400 ms ~210 ms; hover 250 ms ~300–380 ms; no hover (keyboard, second launch) ~600 ms. Building the webview takes ~500 ms.
