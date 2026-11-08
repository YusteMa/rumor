# CLAUDE.md — Rumor

Guía para Claude Code en este repositorio. **Lee `SPEC.md` y `ROADMAP.md` antes de tocar nada.**

## Prioridades por encima de todo
1. **La calidad de sonido es la prioridad número uno**, por encima de cualquier función nueva.
2. **El producto tiene que sentirse premium:** cada detalle visual pulido, nada que parezca hecho deprisa.

## Modo aprendizaje
Mario está aprendiendo a programar con este proyecto.
- Antes de cambiar código, explica qué vas a hacer y por qué.
- Los cambios pequeños los escribe Mario: dale pistas, no la solución.
- Revisa su código como un compañero senior: qué está bien, qué mejorarías.
- Git lo maneja Mario (ramas, commits, PR). No hagas commits por él.

## Qué es
App de bandeja para Windows que genera ruido rosa, marrón y blanco en tiempo real. Tauri v2 + motor de audio en Rust (`cpal`) + panel en TypeScript sin framework. Diseño de referencia: `docs/prototype.html`.

## Entorno
- Windows 10/11, Rust estable (`rustup`), Node LTS y Visual Studio Build Tools con "Desarrollo para el escritorio con C++". WebView2 ya viene en Windows 11.
- Comandos:
  - `npm run tauri dev` — desarrollo
  - `npm run tauri build` — instaladores en `src-tauri/target/release/bundle/`
  - `cd src-tauri && cargo test` — tests del DSP
  - `cargo clippy --all-targets -- -D warnings` y `cargo fmt`
  - `npx tsc --noEmit`

## Estructura
```
rumor/
├─ src/                      # Panel (Vite + TS)
│  ├─ index.html             # panel
│  ├─ settings.html          # ventana de Ajustes (settings.ts, settings-api.ts)
│  ├─ stats.html             # ventana de Estadísticas (stats.ts, stats-api.ts)
│  ├─ mini.html              # mini reloj flotante (mini.ts)
│  ├─ main.ts                # arranque, enlaza eventos ↔ comandos
│  ├─ ui/                    # player.ts, colors.ts, volume.ts, timer.ts, spectrum.ts, accent.ts, radio.ts, footer.ts, charts.ts, study.ts, dial.ts (la esfera)
│  ├─ i18n/                  # es.json, en.json
│  └─ styles/                # tokens.css (tema), controls.css (compartido), panel.css, settings.css, stats.css, mini.css
├─ src-tauri/
│  ├─ src/
│  │  ├─ main.rs
│  │  ├─ lib.rs              # builder de Tauri, plugins, setup
│  │  ├─ state.rs            # AppState compartido
│  │  ├─ commands.rs         # #[tauri::command] expuestos al panel
│  │  ├─ tray.rs
│  │  ├─ tray_glyph.rs      # dibujo del icono de bandeja (puro, testeable)
│  │  ├─ taskbar.rs          # tema claro/oscuro de la barra (tinta del icono)
│  │  ├─ i18n.rs             # textos de Rust, leídos de src/i18n/*.json
│  │  ├─ shortcut.rs
│  │  ├─ update.rs           # auto-actualización: comprobar, avisar, instalar
│  │  ├─ timer.rs
│  │  ├─ ear_break.rs        # recordatorio de descanso de oídos (puro, testeable)
│  │  ├─ settings.rs         # modelo de ajustes y store
│  │  ├─ settings_window.rs  # ventana de Ajustes
│  │  ├─ stats_window.rs     # ventana de Estadísticas
│  │  ├─ mini.rs             # mini reloj flotante: existe solo durante una sesión
│  │  ├─ panel.rs            # crear/posicionar/ocultar ventana, Mica
│  │  └─ audio/
│  │     ├─ mod.rs
│  │     ├─ engine.rs        # stream cpal, reconexión, buffer de 80 ms y underruns al log
│  │     ├─ mmcss.rs         # hilo de audio en MMCSS "Pro Audio"
│  │     ├─ device_watch.rs  # IMMNotificationClient, cascos vs altavoces
│  │     ├─ noise.rs         # generadores (puros, testeables)
│  │     ├─ bell.rs          # campana de aviso: cuenco tibetano sintetizado (puro)
│  │     ├─ smoothing.rs     # rampas y one-pole
│  │     ├─ params.rs        # parámetros atómicos
│  │     └─ analysis.rs      # bandas del espectro para el panel
│  │  └─ study/
│  │     ├─ mod.rs
│  │     ├─ machine.rs       # estados cronómetro/Pomodoro (puro, testeable)
│  │     ├─ idle.rs          # GetLastInputInfo
│  │     ├─ calendar.rs      # ahora en hora local y primer día de la semana
│  │     ├─ stats.rs         # agregación única: panel, Estadísticas y CSV (pura, testeable)
│  │     └─ db.rs            # SQLite: sessions, segments, tags, migraciones reversibles
│  ├─ tests/dsp.rs           # tests espectrales
│  ├─ icons/                 # icono de la app, generado por scripts/gen-app-icon.mjs
│  ├─ capabilities/default.json
│  └─ tauri.conf.json
├─ docs/  (prototype.html, QA.md)
├─ scripts/                  # generadores (icono de la app)
├─ .github/  (workflows/ci.yml, workflows/release.yml, ISSUE_TEMPLATE/)
├─ SPEC.md  ROADMAP.md  README.md  CONTRIBUTING.md  LICENSE  CLAUDE.md
```

## Reglas de oro
1. **Hilo de audio sagrado:** en el callback de `cpal` no hay asignaciones de memoria, locks, logs, `println!`, `unwrap` ni I/O. Comunicación solo por atómicos o por una cola lock-free.
2. **Cero clics:** todo cambio de parámetro se suaviza. Si oyes un clic, es un bug.
3. **El estado vive en Rust.** El panel nunca es la fuente de verdad: pide el estado al abrirse (`get_state`) y escucha el evento `state-changed`.
4. **DSP puro y testeable:** `noise.rs`, `bell.rs` y `smoothing.rs` no dependen de `cpal` ni de Tauri. Se testean renderizando a buffers.
5. **Nada de `unwrap()`/`expect()` fuera de los tests.** Usa errores con `thiserror` y loguea con `log` + `tauri-plugin-log`.
6. **Permisos mínimos** en `capabilities/`: solo los comandos y plugins que se usan.
7. **Sin frameworks de UI ni librerías de componentes.** El panel debe pesar muy poco.
8. **Sin telemetría ni peticiones de red**, salvo la búsqueda de actualizaciones de `update.rs` (activada por defecto y desactivable en Ajustes → Acerca de; decidido el 2026-11-05). No envía nada del usuario.
9. **Diseño = SPEC §4.5.** Todo color, tamaño y duración sale de `src/styles/tokens.css`; ningún valor suelto en los componentes. Sencillo por fuera (panel) y completo por dentro (Ajustes). Si dudas entre añadir algo al panel o a Ajustes, va a Ajustes.
10. **Cascos desconectados → pausa** por defecto (SPEC §3.3). Nunca soltar ruido por los altavoces sin que el usuario lo haya pedido.
11. **El tiempo se mide con `Instant`, no sumando ticks.** El cronómetro debe cuadrar al segundo tras horas, suspensiones y pausas. Las fechas guardadas, en UTC; se muestran en hora local.
12. **Los módulos son independientes.** Sonido y Estudio no se llaman directamente: la vinculación pasa por un coordinador en `state.rs`, para poder desactivar cualquiera sin romper el otro.
13. **Atajos sin AltGr:** ningún atajo por defecto puede producir un carácter en teclados europeos (`Ctrl+Alt` = AltGr).

## Comandos y eventos (contrato)
- Comandos: `get_state`, `toggle`, `set_playing(bool)`, `set_color("pink"|"brown"|"white")`, `set_volume(f32 0..1)`, `set_timer(minutes: u32 | 0)`, `get_settings`, `update_settings(partial)`, `set_shortcut(action, String)`, `quit`.
- Comandos de estudio: `study_toggle`, `study_finish`, `study_set_mode("stopwatch"|"pomodoro")`, `study_set_tag(id|null)`, `study_resolve_idle(keep: bool)`, `study_set_linked(bool)` (solo esta sesión), `study_totals`, `tags_list`, `tags_upsert`, `tags_delete`.
- Eventos de Rust al panel: `state-changed` (estado completo, incluido el del estudio), `spectrum` (`[f32; 64]`, solo con el panel visible), `timer-tick` (segundos restantes), `study-tick` (1 Hz, solo con el panel visible), `study-idle` (minutos inactivo).

## Forma de trabajar
- **Una fase del ROADMAP cada vez.** Al terminar: tests, clippy, fmt, tsc → Mario hace el commit con Conventional Commits (`feat:`, `fix:`, `chore:`…); Claude puede proponerle el mensaje.
- Antes de una decisión de diseño no cubierta por `SPEC.md`, **pregunta**.
- Crates sugeridos (comprueba la última versión estable antes de añadirlos): `cpal`, `fastrand`, `rtrb`, `thiserror`, `log`, `serde`, `window-vibrancy`, `rustfft` (dev). Plugins: `global-shortcut`, `autostart`, `store`, `single-instance`, `positioner`, `notification`, `log`.
- Código, nombres y commits **en inglés**. Textos de la UI con i18n.
