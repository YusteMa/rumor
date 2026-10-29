# Rumor — Hoja de ruta

## Fases de la beta (v0.1.0-beta)

Cada fase termina **compilando, con los tests en verde y con un commit**. No se pasa a la siguiente sin cumplir sus criterios.

### Fase 0 — Esqueleto
- `create-tauri-app` con Vite + TypeScript sin framework, Tauri v2.
- Estructura de carpetas de `CLAUDE.md`, licencia MIT, `.gitignore`, README provisional.
- CI básico en GitHub Actions: `cargo fmt --check`, `clippy`, `cargo test` y `tsc` en `windows-latest`.

**Hecho cuando:** `npm run tauri dev` abre una ventana y el CI pasa.

### Fase 1 — Motor de audio (lo más importante)
- `audio/noise.rs`: generadores blanco, rosa (Kellet refined) y marrón (+HPF), con estado por canal.
- `audio/smoothing.rs`: one-pole para volumen y rampas para fundidos y crossfade.
- `audio/engine.rs`: stream `cpal`, parámetros atómicos, reconstrucción ante errores o cambio de dispositivo.
- Calibrar la ganancia de normalización por color.
- **Tests del DSP de SPEC §6.**

**Hecho cuando:** los tests pasan, suena limpio, no hay clics al cambiar nada y sobrevive a desenchufar los cascos.

### Fase 2 — Bandeja y atajo
- Icono con estado activo e inactivo (y variantes para la barra clara y oscura), tooltip dinámico.
- Clic izquierdo para alternar; clic derecho de momento con un menú provisional (Alternar / Salir).
- Atajo global `Ctrl+Alt+P` con detección de conflictos.
- Instancia única.

**Hecho cuando:** se puede usar la app entera sin ver ninguna ventana.

### Fase 3 — Panel
- Portar el diseño de `docs/prototype.html` a `src/` (TS modular, sin Web Audio: todo vía `invoke` y eventos).
- Ventana sin marco anclada a la bandeja, ocultar al perder el foco, Mica en Win11 y sólido en Win10.
- Tema del sistema y color de acento.
- Espectro: Rust calcula unas 64 bandas logarítmicas a ~30 fps y las emite **solo con el panel visible**.
- Clic derecho → panel (sustituye al menú provisional).

**Hecho cuando:** el panel se abre en menos de 150 ms, se cierra al hacer clic fuera, se ve bien al 100/150/200 % de escalado y con la barra de tareas en cualquier lado.

### Fase 4 — Temporizador, ajustes e idioma
- Temporizador en Rust (sigue funcionando sin panel), con fundido al acabar y notificación.
- Pantalla de ajustes: autoarranque, empezar sonando, acción del clic izquierdo, atajo, idioma.
- `tauri-plugin-store` para la persistencia.
- i18n ES/EN.
- Notificación de bienvenida en el primer arranque.

### Fase 5 — Pulido y release
- Iconos definitivos (app, bandeja e instalador).
- `docs/QA.md` completo y pasado a mano.
- README final con GIF, capturas y aviso de SmartScreen.
- Workflow de release con `tauri-action`: al hacer push de un tag `v*` → pre-release con `.exe` (NSIS) y `.msi`.
- Plantillas de issues y Discussions.
- **Tag `v0.1.0-beta.1`.**

---

## Después de la beta

### v0.2 — "Que no tenga que abrirlo"
- **Controles multimedia de Windows** (SMTC, crate `souvlaki`): aparece en el panel multimedia de Windows 11 y responde a las **teclas play/pausa** del teclado y de los cascos Bluetooth.
- **Auto-actualización** (`tauri-plugin-updater` con firma de Tauri y `latest.json` en Releases).
- **Slider de tono** continuo (filtro *tilt*): de oscuro a brillante sin cambiar de preset.

### v0.3 — "Más sonidos, mismo principio"
- Más colores: gris (ecualizado a la sensibilidad del oído), azul y violeta.
- **Pomodoro** integrado (25/5 con un aviso suave en lugar del corte).
- Pausa automática al empezar una llamada o al reproducir otro audio (opcional; investigar la viabilidad con WASAPI session events).

### v1.0
- Firma de código (SignPath Foundation).
- Publicación en `winget`.
- Builds para macOS y Linux (Tauri lo permite casi gratis).

---

## Cómo medir si la beta va bien
- Issues abiertos y la calidad del feedback en Discussions (no hay telemetría, a propósito).
- Estrellas y descargas de la release en GitHub.
- Sitios donde publicarla: r/Windows11, r/software, r/opensource, r/ADHD (el ruido rosa es muy popular ahí) y, en España, Twitter/X y Menéame.
