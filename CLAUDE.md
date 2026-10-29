# CLAUDE.md — Rumor

Guía para Claude Code en este repositorio. **Lee `SPEC.md` y `ROADMAP.md` antes de tocar nada.**

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
│  ├─ index.html
│  ├─ main.ts                # arranque, enlaza eventos ↔ comandos
│  ├─ ui/                    # player.ts, colors.ts, volume.ts, timer.ts, spectrum.ts, settings.ts
│  ├─ i18n/                  # es.json, en.json
│  └─ styles/                # tokens.css (tema), panel.css
├─ src-tauri/
│  ├─ src/
│  │  ├─ main.rs
│  │  ├─ lib.rs              # builder de Tauri, plugins, setup
│  │  ├─ state.rs            # AppState compartido
│  │  ├─ commands.rs         # #[tauri::command] expuestos al panel
│  │  ├─ tray.rs
│  │  ├─ shortcut.rs
│  │  ├─ timer.rs
│  │  ├─ settings.rs
│  │  ├─ panel.rs            # crear/posicionar/ocultar ventana, Mica
│  │  └─ audio/
│  │     ├─ mod.rs
│  │     ├─ engine.rs        # stream cpal, reconexión
│  │     ├─ noise.rs         # generadores (puros, testeables)
│  │     ├─ smoothing.rs     # rampas y one-pole
│  │     ├─ params.rs        # parámetros atómicos
│  │     └─ analysis.rs      # bandas del espectro para el panel
│  ├─ tests/dsp.rs           # tests espectrales
│  ├─ icons/
│  ├─ capabilities/default.json
│  └─ tauri.conf.json
├─ docs/  (prototype.html, QA.md)
├─ .github/  (workflows/ci.yml, workflows/release.yml, ISSUE_TEMPLATE/)
├─ SPEC.md  ROADMAP.md  README.md  LICENSE  CLAUDE.md
```

## Reglas de oro
1. **Hilo de audio sagrado:** en el callback de `cpal` no hay asignaciones de memoria, locks, logs, `println!`, `unwrap` ni I/O. Comunicación solo por atómicos o por una cola lock-free.
2. **Cero clics:** todo cambio de parámetro se suaviza. Si oyes un clic, es un bug.
3. **El estado vive en Rust.** El panel nunca es la fuente de verdad: pide el estado al abrirse (`get_state`) y escucha el evento `state-changed`.
4. **DSP puro y testeable:** `noise.rs` y `smoothing.rs` no dependen de `cpal` ni de Tauri. Se testean renderizando a buffers.
5. **Nada de `unwrap()`/`expect()` fuera de los tests.** Usa errores con `thiserror` y loguea con `log` + `tauri-plugin-log`.
6. **Permisos mínimos** en `capabilities/`: solo los comandos y plugins que se usan.
7. **Sin frameworks de UI ni librerías de componentes.** El panel debe pesar muy poco.
8. **Sin telemetría ni peticiones de red** (salvo el futuro updater, que será opt-in).

## Comandos y eventos (contrato)
- Comandos: `get_state`, `toggle`, `set_playing(bool)`, `set_color("pink"|"brown"|"white")`, `set_volume(f32 0..1)`, `set_timer(minutes: u32 | 0)`, `get_settings`, `update_settings(partial)`, `set_shortcut(String)`, `quit`.
- Eventos de Rust al panel: `state-changed` (estado completo), `spectrum` (`[f32; 64]`, solo con el panel visible), `timer-tick` (segundos restantes).

## Forma de trabajar
- **Una fase del ROADMAP cada vez.** Al terminar: tests, clippy, fmt, tsc → commit con Conventional Commits (`feat:`, `fix:`, `chore:`…).
- Antes de una decisión de diseño no cubierta por `SPEC.md`, **pregunta**.
- Crates sugeridos (comprueba la última versión estable antes de añadirlos): `cpal`, `fastrand`, `rtrb`, `thiserror`, `log`, `serde`, `window-vibrancy`, `rustfft` (dev). Plugins: `global-shortcut`, `autostart`, `store`, `single-instance`, `positioner`, `notification`, `log`.
- Código, nombres y commits **en inglés**. Textos de la UI con i18n.
