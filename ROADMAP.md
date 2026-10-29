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
- `audio/device_watch.rs`: `IMMNotificationClient` + tipo de salida (cascos/altavoces) → política de SPEC §3.3 (**por defecto: pausar al pasar a altavoces**).
- Calibrar la ganancia de normalización por color.
- **Tests del DSP de SPEC §6.**

**Hecho cuando:** los tests pasan, suena limpio, no hay clics al cambiar nada y **al quitar los cascos (con cable y Bluetooth) se pausa**; con "Seguir sonando" continúa por los altavoces.

### Fase 2 — Bandeja y atajo
- Icono con estado activo e inactivo (y variantes para la barra clara y oscura), tooltip dinámico.
- Clic izquierdo para alternar; clic derecho de momento con un menú provisional (Alternar / Salir).
- Atajo global `Ctrl+Alt+P` con detección de conflictos.
- Instancia única.

**Hecho cuando:** se puede usar la app entera sin ver ninguna ventana.

### Fase 3 — Panel
- Crear `src/styles/tokens.css` con los tokens de SPEC §4.5 **antes** de maquetar nada.
- Portar la estructura de `docs/prototype.html` a `src/` aplicando el **sistema de diseño de SPEC §4.5** (los colores del prototipo se sustituyen por los tokens nuevos). TS modular, sin Web Audio: todo vía `invoke` y eventos.
- Animaciones de §4.5 (apertura, morph de play, halo, transición del espectro) y modo de movimiento reducido.
- Todos los estados de §4.5, incluido "Pausado: se desconectaron los cascos".
- Ventana sin marco anclada a la bandeja, ocultar al perder el foco, Mica en Win11 y sólido en Win10.
- Tema del sistema y color de acento.
- Espectro: Rust calcula unas 64 bandas logarítmicas a ~30 fps y las emite **solo con el panel visible**.
- Clic derecho → panel (sustituye al menú provisional).

**Hecho cuando:** el panel se abre en menos de 150 ms, se cierra al hacer clic fuera, se ve bien al 100/150/200 % de escalado y con la barra de tareas en cualquier lado.

### Fase 3b — Módulo Estudio
- `study/`: máquina de estados (parado → contando → en pausa → terminado; y en Pomodoro, trabajo ↔ descanso), basada en `Instant` (nunca sumando ticks, para que no derive).
- SQLite (`rusqlite` con `bundled`): tabla `sessions` y tabla `tags`, con migraciones versionadas desde el principio.
- Vinculación con el ruido, inactividad opcional con `GetLastInputInfo` y la pregunta al volver.
- Tarjeta de Estudio en el panel, totales de hoy y de la semana, atajo `Ctrl+Alt+T`.
- Módulos activables (SPEC §4.3c) e icono y tooltip adaptados.
- Tests: la máquina de estados (pausas, fases del Pomodoro, sesión que cruza la medianoche) y que la suma diaria sea correcta.

- Avisos configurables (notificación / sonido suave "blim" sintetizado / las dos / nada) para el fin del temporizador, el Pomodoro y el descanso de oídos.
- Recordatorio de descanso de oídos tras X horas con cascos (por defecto 3 h, desactivable).
- El cronómetro se pausa al bloquear el PC y no cuenta el tiempo suspendido.

**Hecho cuando:** dejas el cronómetro 2 h con pausas y el total coincide al segundo con un reloj; el Pomodoro cambia de fase sin intervención; desactivar un módulo lo hace desaparecer por completo; el "blim" suena limpio encima del ruido sin cortarlo.

### Fase 4 — Temporizador, ajustes e idioma
- Temporizador en Rust (sigue funcionando sin panel), con fundido al acabar y notificación.
- **Ventana de Ajustes** con todas las secciones de SPEC §4.4, cambios al instante y el mismo sistema de diseño que el panel.
- `tauri-plugin-store` para la persistencia, más **exportar/importar ajustes en JSON**.
- i18n ES/EN.
- Notificación de bienvenida en el primer arranque.

- **Volumen por salida:** un volumen recordado para cada dispositivo, aplicado con suavidad al cambiar.
- **Bloqueo del PC:** pausa al bloquear (Win+L) y, al desbloquear, reanuda solo si la salida sigue siendo de cascos. Configurable en Reproducción.
- **Modo mono** en Sonido, para un solo auricular.

### Fase 5 — Pulido y release
- Iconos definitivos (app, bandeja e instalador).
  - Revisar el cambio de icono de la bandeja según el feedback de la prueba de la Fase 2.
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
- **Estadísticas de estudio:** gráfico semanal y mensual, tiempo por asignatura, racha de días y exportar a CSV.

- **Reacción a otro audio** (vídeos, llamadas): bajar el volumen (cuánto, configurable) / pausar / nada, con transiciones suaves como en iOS. Se detecta con las sesiones de audio de WASAPI.

### v0.3 — "Más sonidos, mismo principio"
- Más colores: gris (ecualizado a la sensibilidad del oído), azul y violeta.

### v1.0
- Firma de código (SignPath Foundation).
- Publicación en `winget`.
- Builds para macOS y Linux (Tauri lo permite casi gratis).

---

## Cómo medir si la beta va bien
- Issues abiertos y la calidad del feedback en Discussions (no hay telemetría, a propósito).
- Estrellas y descargas de la release en GitHub.
- Sitios donde publicarla: r/Windows11, r/software, r/opensource, r/ADHD (el ruido rosa es muy popular ahí) y, en España, Twitter/X y Menéame.
