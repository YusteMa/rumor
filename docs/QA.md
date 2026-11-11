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
- [ ] Left click: noise fades in and the bars rise into the "waves" in about 0.4 s, then keep still; left click again: they settle back to the idle icon. With Settings → Accessibility → Visual effects → Animation effects off, the icon switches at once.
- [ ] Tooltip while playing: `Rumor · Rosa · 40 %` (English: `Rumor · Pink · 40%`); paused: `Rumor · En pausa`.
- [ ] Unplug / switch off headphones while playing: idle icon, tooltip `Rumor · En pausa: se desconectaron los cascos`.
- [ ] `Ctrl+Alt+P` toggles from any app, including a full-screen one.
- [ ] Spanish keyboard: typing with AltGr in an editor (`AltGr+E` = €, `AltGr+2` = @) still works and never toggles Rumor.
- [ ] Shortcut taken: register `Ctrl+Alt+P` in another app first (e.g. an AutoHotkey script `^!p::return`), start Rumor → notification says it uses `Ctrl+Alt+N`, and the panel footer says so.
- [ ] Launch Rumor a second time: no second icon; the panel opens.
- [ ] Switch Windows between light and dark mode: the icon ink follows within a second.
- [ ] Display scaling 100 / 125 / 150 / 200 %: the icon is crisp, drawn at its exact size (restart Rumor after changing it).
- [ ] Task Manager: Rumor with no panel uses ~20 MB of RAM and ~0 % CPU while paused.

## Panel (phase 3)

Reference screenshots (from the mock backend): `docs/screenshots/panel-light.png`, `panel-dark.png`, `panel-study.png`, `settings-light.png`.

- [ ] Right click on the tray icon: the panel fades in, 8 px upward, in under 150 ms, next to the icon.
- [ ] Click anywhere outside: it fades out. Right click on the icon while open: it closes and stays closed.
- [ ] Real clicks, not a script: right click opens the panel and it stays open, with the hover first and with a fast click straight onto the icon. Ten right clicks in a row alternate open / closed every time. Alternating left and right clicks: left toggles the noise (closing the panel if open), right opens it. Also with the icon inside the hidden-icons (^) flyout, which is where Windows 11 puts new icons: Explorer takes the focus back later there. The panel must never come up empty.
- [ ] Taskbar at the bottom, top, left and right (and auto-hidden): the panel sits next to it, 12 px from the edges, never off-screen.
- [ ] Two monitors with different scaling: the panel opens on the monitor with the tray, crisp and the right size.
- [ ] Windows 11: Mica behind the panel, rounded corners, a warm 1 px border in light and dark. Windows 10: solid colour, no glitches.
- [ ] Windows light and dark mode: the panel follows the system.
- [ ] Play: the icon morphs into pause, the halo breathes slowly (4 s), the status dot turns pink, the spectrum fades in.
- [ ] Pink / brown / white: the curve morphs to the new slope (−3 / −6 / 0 dB/oct) and lies on the dashed line.
- [ ] The accent follows the noise (dusty pink, toasted caramel, blue-grey pearl) on play, halo, curve, selection, slider and focus ring, crossfading in ~300 ms together with the curve. Text stays neutral; the tray icon stays monochrome.
- [ ] Panel lifecycle: hover the tray icon for half a second, then right click: the panel appears at once. Close it and wait 5 minutes: Task Manager shows Rumor back near 5 MB (WebView2 processes gone). Hovering again rebuilds it.
- [ ] Volume drag is smooth, the value follows the thumb, the tray tooltip follows the value.
- [ ] Timer 25 min while playing: "quedan 24:59" counts down here and in the tooltip; at the end the noise fades out over ~10 s.
- [ ] Timer and colour: with 25 min counting down, switch pink → brown → white: the countdown carries on. Pause: it freezes (also in the tooltip); play: it continues from there. Only picking a time (or ∞) restarts it.
- [ ] Unplug headphones while the panel is open: "En pausa: se desconectaron los cascos" under the title.
- [ ] No output device at all: the play button is disabled and the panel says there is no audio output.
- [ ] Keyboard only: Tab reaches every control with a visible pink focus ring; arrows move within Color and Timer; Space plays/pauses; Esc closes.
- [ ] Windows "Animation effects" off (reduced motion): no halo and no movement, only fades.
- [ ] Scaling 100 / 150 / 200 %: text and the spectrum are crisp, nothing is clipped.
- [ ] Footer: "Salir" fades the noise out and quits; "Ajustes" says it arrives in the next version.

## Settings (phase 4)

Open it from the panel (⚙ Ajustes). Every change must apply at once, without a Save button.

- [ ] First launch ever (delete `%APPDATA%\io.github.yustema.rumor\settings.json`): a welcome notification; not on the next launch.
- [ ] Quit and reopen: colour, volume and timer are the same as before.
- [ ] Sound: "Color al iniciar" fixed to Marrón starts brown; fade length 3 s is audibly slower; maximum volume 50 % makes the panel's 100 % as loud as 50 % was.
- [ ] Playback: each "Si se desconectan los cascos" option behaves as in the phase 1 table; "Al iniciar Rumor": No sonar / Sonar siempre / Como lo dejé (quit while playing, reopen: plays).
- [ ] Timer: change the quick times to 10/20/30 → the panel's buttons change; final fade 5 s (Avanzado → Temporizador); at the end, the alert chosen in General → Avisos.
- [ ] Shortcuts: record Ctrl+Shift+F9 for "Subir volumen" and use it; Ctrl+Alt+E is refused on a Spanish keyboard (types €); the same combination for two actions is refused; Esc cancels; Delete clears.
- [ ] Atajos → clic izquierdo "Abrir el panel" opens it instead of toggling.
- [ ] Appearance: theme light/dark/system changes the panel and Settings at once; accent "El de Windows" uses Windows' colour, still readable; Mica off (Avanzado → Ventana) gives solid backgrounds; spectrum off hides the card; Windows "Animation effects" off removes the halo; language English switches every text, tray tooltip included.
- [ ] General: "Iniciar con Windows" on → Rumor starts after signing in again; off → it doesn't.
- [ ] Advanced: pick a fixed output → noise moves there; unplug it → pauses, never falls back to the speakers; buffer Automatic (80 ms)/20/50/100 ms plays clean; export then import gives the same settings; a non-settings JSON is refused with a message; reset asks first and keeps colour/volume/timer; "Abrir carpeta" opens the logs.
- [ ] Settings has six sections: General, Estudio, Atajos, Apariencia, Avanzado (grouped under Sonido, Temporizador, Salida de audio, Ventana, Mantenimiento) and Acerca de.
- [ ] Settings looks right in light and dark, at 100/150/200 % scaling, and with the keyboard only.
- [ ] Dropdowns (no native lists): open with a click, Enter, Space or ↓; ↑ ↓ Home End move; a letter jumps (again cycles); Enter or Tab chooses; Esc closes without changing; a click outside closes. The list is opaque and readable in light and dark, with Mica on and off, and opens upwards near the bottom.
- [ ] Window size: at 200 % on a 1080p screen it opens fully on screen; it can't be made smaller than 520 × 440. Narrow: the dropdowns move under their explanation instead of squeezing it; wide: the content stops at 720 px and the explanations keep a reading width.
- [ ] Volume per output: 30 % on headphones, switch to speakers and set 60 %, switch back: 30 % again, smoothly (no jump or click); with "Volumen de cada salida" off the volume stays put.
- [ ] Lock (Win+L) while playing on headphones: it pauses; unlock: it plays again. Lock on headphones, take them off, unlock: stays paused, nothing from the speakers. "Pausar": stays paused after unlock. "No hacer nada": keeps playing while locked. Paused before locking: unlock never starts it.
- [ ] Mono: one earbud in, mono on: the noise sounds the same and just as loud; switching mono on and off while playing never clicks.

## Study (phase 3b)

Turn it on in Ajustes → Estudio → Modo estudio. The panel grows to make room for the study card.

- [ ] Off (the default): no study card, no dot on the tray icon, nothing about it in the tooltip, and Ctrl+Alt+T does nothing. Turning it on brings all of it; turning it off mid-session stores the session and removes everything again.
- [ ] Linked (default): play on the noise starts the stopwatch and play on the stopwatch starts the noise; pause on either pauses both. Unplugging the headphones, locking the PC (Win+L) and sleeping pause both. The sleep timer ending pauses the stopwatch too.
- [ ] Not linked: each one plays and pauses on its own. Locking or sleeping still pauses the stopwatch; unlocking continues it.
- [ ] Link button beside «Estudio» (linked by default): closed and in the accent. Click it: the halves draw apart, it turns grey, and the tooltip says they go their own way. «Empezar» now counts in silence; play on the noise doesn't start the stopwatch; Ctrl+Alt+P and Ctrl+Alt+T each move only their own. Settings → Estudio still says «Vincular con el ruido» on.
- [ ] Join them again mid-session: with the stopwatch running the noise fades in; with only the noise playing the stopwatch starts. In a Pomodoro break set to «Pausar» the noise waits for the next work block.
- [ ] «Terminar sesión» while apart doesn't stop the noise, and the link closes again (back to Settings). The same with «Vincular con el ruido» off: the link starts open, closing it joins them for one session only.
- [ ] Two hours with pauses: the session time matches a clock to the second, and "Hoy" goes up by exactly the work time.
- [ ] Pomodoro with 1-minute work and break (Settings): it changes phase on its own; "Trabajo 1/4 · 0:42" in the panel, the ring fills, and the tooltip shows "Descanso 0:30" during breaks. With "Bajar a la mitad" the noise glides down and back up, with "Pausar" it fades out and back in, with "Seguir igual" it doesn't change. Break time doesn't count.
- [ ] Alerts: "Campana" plays a soft singing-bowl "bong" over the noise without cutting it, also while the noise is paused; "Notificación" shows only the notification; "Las dos" both; "Nada" nothing. This covers the sleep timer end, Pomodoro phases and the ear break.
- [ ] Idle (Inactividad 10 min): leave the keyboard and mouse for 11 minutes while studying, then move the mouse: a dot on the tray icon and "¿Seguías estudiando?" in the tooltip; the panel stays closed and nothing steals the focus. Opening the panel shows the question; "No, descontar" takes those minutes out of the session and of "Hoy"; "Sí, contar" keeps them. Unanswered at the end of the session, they count.
- [ ] Tags: create "Cálculo" and "EDA" in Settings, recolour one, rename one; a duplicate name is refused. The panel shows a tag picker only once a tag exists; deleting a tag keeps its sessions.
- [ ] Quit Rumor mid-session and open it again: that session is in "Hoy". Kill the process mid-session: at most a minute is lost.
- [ ] Ear break (General, every 3 h): after 3 h playing on headphones, one alert; the noise keeps playing. A 15-minute pause starts the count again.
- [ ] A session across midnight counts on each day only for its own part.

## Release (phase 5)

Install from the GitHub release on a clean user account (or a VM), not from `tauri dev`.

- [ ] The setup .exe shows SmartScreen's "Windows protected your PC"; More info → Run anyway installs without asking for admin rights. The installer speaks Spanish on a Spanish Windows and English otherwise.
- [ ] The app icon (three bars on a dark tile with a fine rim) looks crisp in the installer, Start menu, Settings → Apps, Alt+Tab with Settings open, and the taskbar, both in light and dark mode, at 100 % and 150 %.
- [ ] Settings → About says "Versión 0.1.0-beta.5".
- [ ] Uninstall from Settings → Apps: the tray icon goes away and its folder in `%LOCALAPPDATA%\Rumor` is gone; settings and study history stay in `%APPDATA%\io.github.yustema.rumor` (on purpose, for reinstalling).
- [ ] The .msi installs the same app.
- [ ] README: the GIF plays, the screenshots load and the links work on github.com.

## Updates

Needs two releases, the second with a higher version (e.g. install beta.2 and publish beta.3).

- [ ] Settings → About → Updates: "Buscar actualizaciones" is on; "Buscar ahora" says "Buscando…" and then "Tienes la última versión."
- [ ] With the network off, "Buscar ahora" says it couldn't check, and nothing else changes.
- [ ] After the newer release is out, within 30 s of opening Rumor (or 6 h of leaving it open): a notification "Hay una versión nueva de Rumor", once; restarting Rumor doesn't repeat it.
- [ ] The panel footer shows "↑ Actualizar" instead of the shortcut, and the panel keeps its height. Hovering shows the version.
- [ ] While playing, press "Actualizar": the arrow pulses while it downloads (the noise keeps playing), then the panel closes, the noise fades out without a click, a small installer window shows its progress, and Rumor opens again by itself **playing**. Unplug the headphones while it updates: it opens paused (SPEC §3.3).
- [ ] With the stopwatch running, the session is in today's total after the update.
- [ ] No ghost tray icon is left behind after the update.
- [ ] Settings → About says the new version.
- [ ] With "Buscar actualizaciones" off, nothing is checked (log: no "update check" lines after launch).

## Study card and mini clock (SPEC §4.3b)

- [ ] Before a session: the sphere reads 0:00 (or the Pomodoro block, 25:00) with the mode below, and the column shows Cronómetro/Pomodoro and «Empezar».
- [ ] Pomodoro: the arc and its knob fill the phase smoothly, second by second; «Trabajo 2 de 4 · Sesión de …». A break turns them grey, readable in dark mode.
- [ ] Stopwatch: the trail goes round once a minute; hours («1:23:45») keep their margin to the ring.
- [ ] Paused: arc and knob at half strength, time grey, «Seguir»; the same in the mini clock.
- [ ] The sphere starts on the panel's left edge (with «Estudio», colour and volume); the finish button ends on the right edge, under the tag picker's; nothing shifts between states, in Spanish and English.
- [ ] On 1366×768 at 100 % (Windows 11 and 10) the panel uses the compact spacing and fits without scrolling; on a normal screen it looks exactly as before. Only a shorter screen still makes it scroll; no part ever goes off screen.
- [ ] The pop-out button sits beside the link, same size. Without a session it is dimmed, does nothing and its tooltip says to start a session.
- [ ] In a session, clicking it brings the mini clock out top right and the icon turns accent (arrow nudged out); clicking again puts it away with a fade. Tooltips in Spanish and English.
- [ ] The mini clock never appears on its own: starting a session doesn't show it, and after ending a session with it out, the next session starts without it.
- [ ] Settings → Study no longer has «Mini reloj flotante»; a settings.json saved with `"miniClock": true` loads fine and the clock doesn't show by itself.
- [ ] Dragging moves it and the spot is remembered after restarting Rumor; unplugging that screen brings it back to the corner.
- [ ] Clicking its sphere pauses/resumes without taking the focus from the app you are typing in; clicking the time opens the panel.
- [ ] The × (on hover) puts it away and the panel's button goes back to grey. It never shows in the taskbar or Alt+Tab.
- [ ] Light/dark and the noise colour change it live.

## Statistics (SPEC §4.3d)

Run these on a copy of a real history if you can (`%APPDATA%\io.github.yustema.rumor\study.sqlite3`).

- [ ] The first launch with this version leaves `study.sqlite3.v1.bak` next to the history, and every old session is still there (Sesiones).
- [ ] The study card's "Estadísticas" opens the window and closes the panel; a second click brings the same window to the front.
- [ ] "Hoy" and "Semana" in the panel match the day header in Sesiones and the week total, also with a session running.
- [ ] With a session running, its time is in the total and today's bar, but "Sesiones", "Sesión media", the records and "Así ha ido" leave it out until it ends.
- [ ] A session from 23:30 to 00:30 counts on the day it started, in the panel, the bars and the list.
- [ ] Monday morning with little studied last Monday: the pill shows minutes ("+25 min"), not a percentage. Later in the week it shows "… % vs semana pasada" and "hasta el <día> <hora>".
- [ ] Hovering a bar dims the others and shows the day's total and tags; Tab reaches each bar.
- [ ] "Así ha ido" shows at most three sentences, at most one ▼, none repeating the pill, the longest session or the days studied. The one about the usual week names its base ("tu media de las N semanas anteriores").
- [ ] On a Monday, a subject not studied yet this week never reads "lleva dos semanas bajando" because of it.
- [ ] Paging weeks with ‹ ›: the hour scale stays at 6 h (it grows only for a week above that), so bars compare.
- [ ] Mes: today has an outline, future days have no colour, "Días con estudio" reads "N de M".
- [ ] Sesiones: the running one reads "HH:MM – ahora" and has no edit or delete buttons.
- [ ] Editing a session to overlap the next one says "Se solapa con la sesión de las HH:MM"; shortening it moves only its end. "h:mm" shows next to Duración, and "90" is read as 1:30.
- [ ] Editing a session past the longest one updates "Sesión más larga" and "Mejor día" at once; deleting it and Deshacer put them back.
- [ ] Deleting shows "Sesión borrada · Deshacer"; Deshacer brings it back; after restarting Rumor a deleted one is gone.
- [ ] "+ Añadir sesión" with nothing recorded yet adds a past session; in the future or overlapping, it refuses.
- [ ] Exportar a CSV: opens in Excel with accents right, one row per session, the running one marked; its seconds add up to the totals.
- [ ] Changing the noise colour, the language or Mica updates the open window.
- [ ] Light and dark, Mica on and off, 100 % and 150 %: secondary text and calendar numbers stay legible; the chosen tab is clear (accent line) and "Sin etiqueta" stands out from the card.

### Goal, streak and trend (second delivery)

- [ ] Settings → Study → «Objetivo semanal» at «Sin objetivo»: no ring, no dashed goal line, and the fourth figure is «Días con estudio».
- [ ] With a goal (e.g. 12 h): the ring beside the total shows the percentage and «de 12 h», closes at 100 % and stays closed above it; the bars get a dashed «Objetivo diario» line (goal ÷ 7) whose label reads over a tall bar; changing the goal in Settings updates the open window at once.
- [ ] «Racha» counts weeks in a row that met the goal: the current week adds as soon as it gets there and an unfinished week never breaks it.
- [ ] «Últimas 8 semanas»: the shown week in the accent, a dashed line at «Tu media»; hover or Tab shows each week's dates and total; a click (or Enter) opens that week. Hidden with fewer than two weeks of study.
- [ ] «Por asignatura» (Week): ▲ green / ▼ grey with the difference against the previous week, nothing under 5 min, never red; the tooltip says what it compares with.
- [ ] Changing tab or week: cards fade in, bars rise one after another, the ring fills and the total counts up. The minute save of a session in progress changes the numbers without replaying any of it. With Windows animations off, only the fade.
- [ ] The heat map shows the floating label on hover (no system tooltip). Light and dark.

## Measurements (SPEC §3.4)
- [ ] `cargo run --release --example engine -- --check 20`: frames ≈ 100 % of expected, CPU < 1 % of the machine, buffer 80 ms, 0 underruns.
- [ ] Busy PC: with every core loaded (a big build, or a CPU stress test), the noise stays clean. The log shows `audio thread: MMCSS "Pro Audio"` and `device buffer: … (80.0 ms)`, and no `audio underrun` lines. Measured 2026-11-03 with 32–64 busy threads on 16: before (22 ms, normal priority) 2 underruns/15 s in release and 12–15 in debug; after, 0 in both.
- [ ] Memory (RAM = private working set incl. WebView2, bundled build), measured 2026-10-29: no panel ~3 MB; panel open ~154 MB; hidden and suspended ~148 MB; 5 minutes after closing (webview destroyed) ~6 MB. Executable: 6.0 MB.
- [ ] Panel open latency, click to first frame on screen (2026-10-29): already built 63–78 ms; hover ≥ 600 ms before the click ~100 ms; hover 400 ms ~210 ms; hover 250 ms ~300–380 ms; no hover (keyboard, second launch) ~600 ms. Building the webview takes ~500 ms.
