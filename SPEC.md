# Rumor — Especificación del producto

> Generador de ruido rosa, marrón y blanco **calculado en tiempo real** para Windows.
> Vive en la bandeja del sistema, se activa con un clic o un atajo y no reproduce ningún archivo de audio.

Nombre: **Rumor**. Versión objetivo de este documento: **v0.1.0-beta**.

---

## 1. El problema y la promesa

- El ruido rosa de YouTube o Spotify es **audio comprimido** (AAC/Opus). El ruido es aleatorio e incompresible, así que el códec lo degrada: suena burbujeante o metálico. Además suele ser un **bucle**, y el cerebro acaba detectando la repetición.
- El de iOS (Sonidos de fondo) suena limpio porque **se genera en el momento**.
- **Promesa de Rumor:** el mismo sonido limpio que el iPhone, en Windows, a **un clic o un atajo** de distancia, sin anuncios, sin cuenta, sin telemetría y open source.

**Usuario objetivo:** gente que estudia o trabaja con ruido de fondo (estudiantes, programadores, gente con TDAH), en Windows 10 y 11.

---

## 2. Arquitectura (decidida)

| Capa | Tecnología | Por qué |
|---|---|---|
| Shell de la app | **Tauri v2** | Binario de unos 5–10 MB, usa WebView2 (ya viene en Windows), bandeja y atajos nativos |
| Motor de audio | **Rust + `cpal`** (WASAPI) | El audio sigue sonando con el panel cerrado, tiene latencia estable, gasta muy poca CPU y no depende del WebView |
| Interfaz (panel) | **Vite + TypeScript sin framework** | El panel es pequeño y un framework sería peso muerto. Reutiliza el diseño de `docs/prototype.html` |
| Persistencia | `tauri-plugin-store` | JSON en `%APPDATA%` |

**Principio clave:** la UI es solo un **mando a distancia**. Todo el estado vive en Rust; la UI envía comandos y recibe eventos.

```
┌──────────── Rust (proceso principal) ─────────────┐
│  AppState (Arc<…>)                                │
│   ├─ AudioEngine ── hilo de audio cpal (WASAPI)   │
│   │     └─ parámetros vía atómicos (sin locks)    │
│   ├─ Tray (icono + tooltip + clics)               │
│   ├─ GlobalShortcut                               │
│   ├─ Timer (apagado programado)                   │
│   └─ Settings (store)                             │
│        ▲ commands          │ events               │
└────────┼───────────────────┼──────────────────────┘
         │                   ▼
   Panel WebView2 (HTML/TS) — se crea al abrir, se oculta al perder el foco
```

### Descartado (y por qué)
- **Electron:** más de 150 MB para hacer ruido. No.
- **PWA:** no puede poner icono en la bandeja ni registrar atajos globales.
- **Widgets de Windows 11:** solo admiten tarjetas tipo Adaptive Cards, requieren app empaquetada y casi nadie abre ese panel.
- **Audio en el WebView (AudioWorklet):** se cortaría al destruir la ventana y depende del throttling de WebView2.

---

## 3. Motor de audio (especificación técnica)

### 3.1 Generación
- **Blanco:** PRNG rápido (`fastrand` o xorshift128+ propio) → uniforme en [−1, 1].
- **Rosa:** filtro *refined* de **Paul Kellet** (7 polos, error de ±0,05 dB por encima de 9,2 Hz a 44,1 kHz). **Estado independiente por canal**, lo que da un estéreo realmente decorrelado y una sensación de amplitud.
- **Marrón:** integrador con fuga `y = (y + 0.02·w) / 1.02`, más un **filtro paso alto a ~20 Hz** para eliminar el DC y la deriva.
- Funciona a la **frecuencia de muestreo nativa del dispositivo** (48 kHz normalmente). Nada de remuestrear.

### 3.2 Calidad (lo que lo hace "pulido")
- **Normalización de sonoridad entre colores:** a mismo volumen, rosa, marrón y blanco deben **sonar igual de fuertes**. Calibrar una ganancia por color con RMS, y como objetivo con ponderación K (tipo LUFS) a ±1 dB.
- **Sin clics nunca:**
  - Fundido de entrada y salida de unos **400 ms** (curva exponencial).
  - Cambios de volumen suavizados **muestra a muestra** (one-pole, unos 30 ms).
  - Cambio de color con **crossfade** de unos 150 ms (se generan ambos durante la transición).
- **Margen de seguridad:** pico ≤ −3 dBFS y un *soft clip* (tanh) como red de seguridad.
- **Volumen perceptual:** `gain = v²` o una curva en dB (−60 a 0 dB) sobre el slider del 0 al 100.

### 3.3 Robustez en Windows
- **Cambio de dispositivo** (enchufas cascos, Bluetooth, HDMI): detectar el error del stream y **reconstruirlo en el nuevo dispositivo por defecto** en menos de 1 s, retomando el sonido.
- **Suspensión e hibernación:** al volver, reabrir el stream si murió.
- **Hilo de audio sin asignaciones de memoria, sin locks y sin logs.** Los parámetros se pasan con `AtomicU32` (bits de f32) o con un `rtrb`/`ringbuf` para los comandos.

### 3.4 Objetivos medibles
- CPU con el audio activo: **< 1 %** en un portátil moderno.
- RAM con el panel cerrado: **medir y documentar** (WebView2 es el coste principal; destruir el panel al cerrarlo si compensa).
- Arranque hasta que se ve el icono: **< 1 s**.

---

## 4. Interacción

### 4.1 Bandeja del sistema (el "widget")
| Acción | Comportamiento por defecto | Configurable |
|---|---|---|
| **Clic izquierdo** | Activa o para el ruido | Sí: "alternar" / "abrir panel" |
| **Clic derecho** | Abre el **panel** junto al icono | — |
| **Pasar el ratón** | Tooltip: `Rumor · Rosa · 40 % · quedan 23:10` | — |

- **Icono con dos estados:** activo (con ondas) e inactivo (atenuado). Variantes para la barra clara y la oscura.
- Al **arrancar sin configuración**, mostrar una notificación de Windows: "Rumor está en la bandeja. Clic para activar, clic derecho para opciones".

### 4.2 Panel
- Ventana **sin marco**, de unos 360×480 px, **anclada a la bandeja** (`tauri-plugin-positioner`), siempre encima y fuera de la barra de tareas.
- Se **oculta al perder el foco** (como el panel de volumen de Windows).
- Efecto **Mica o Acrylic** en Windows 11 (`window-vibrancy`), con color sólido como alternativa en Windows 10.
- Sigue el **tema claro u oscuro de Windows** y su color de acento (opcional).
- Contenido (basado en `docs/prototype.html`):
  1. Botón grande de play/pausa.
  2. Color: **Rosa / Marrón / Blanco**.
  3. Volumen.
  4. Temporizador: **∞ / 25 / 50 / 90 min**, con cuenta atrás visible.
  5. Espectro en vivo (Rust envía unos 30 frames/s de bandas por evento; **solo con el panel visible**).
  6. Pie: ⚙ Ajustes · Salir.
- **Accesible:** navegable con teclado, foco visible, `Espacio` alterna y `Esc` cierra.

### 4.3 Atajo global
- Por defecto **`Ctrl + Alt + P`** para alternar. Si está ocupado, avisar y proponer otro.
- Configurable en los ajustes (capturar la combinación pulsándola).

### 4.4 Ajustes
- Iniciar con Windows (`tauri-plugin-autostart`), **desactivado por defecto**.
- Empezar sonando al iniciar (sí/no).
- Acción del clic izquierdo.
- Atajo global.
- Idioma: **Español / English**, detectado del sistema.
- Recordar el último color, el volumen y el temporizador (siempre).

---

## 5. Alcance de la beta (v0.1.0-beta)

✅ **Entra**
- Motor Rust: rosa, marrón y blanco; estéreo decorrelado; normalización; fundidos; crossfade.
- Reconexión ante el cambio de dispositivo.
- Bandeja con dos estados, tooltip y clics.
- Panel con el diseño del prototipo, Mica, tema del sistema y cierre al perder el foco.
- Atajo global configurable.
- Temporizador con fundido de salida.
- Iniciar con Windows (opcional).
- Ajustes persistentes.
- **Instancia única** (`tauri-plugin-single-instance`): si se abre otra vez, se muestra el panel.
- ES/EN.
- Instalador NSIS (`.exe`) y `.msi` generados con **GitHub Actions** (`tauri-apps/tauri-action`) y publicados como **pre-release**.

❌ **Fuera de la beta** (va en ROADMAP.md)
- Auto-actualización, firma de código, controles multimedia, ecualizador de tono, mezclador de sonidos, macOS y Linux.

---

## 6. Calidad y tests

- **Tests unitarios del DSP** (sin dispositivo de audio, renderizando a buffer):
  - La pendiente del rosa debe ser **−3 dB/octava ±0,5** entre 50 Hz y 10 kHz (FFT con `rustfft` en dev-deps, promediando unos 60 s de señal).
  - Marrón: **−6 dB/oct ±0,7**. Blanco: **0 ±0,5**.
  - Ningún NaN ni infinito tras 10 minutos simulados; pico < 1,0.
  - RMS ponderado entre colores con una diferencia **≤ 1 dB**.
  - Correlación entre los canales izquierdo y derecho **< 0,05**.
- `cargo clippy -- -D warnings` y `cargo fmt --check` limpios.
- Frontend: `tsc --noEmit` limpio.
- **Checklist manual** antes de cada release: `docs/QA.md` (cascos ↔ altavoces, suspensión, atajo ocupado, tema claro y oscuro, escalado al 100/150/200 %, varios monitores, barra de tareas en otra posición).

---

## 7. Distribución

- **GitHub Releases** como canal principal; la beta va marcada como *pre-release*.
- **SmartScreen:** sin firmar, Windows mostrará "Windows protegió su PC". En la beta se documenta en el README ("Más información → Ejecutar de todas formas"). Opciones para la v1:
  - **SignPath Foundation**: firma gratuita para proyectos open source (requiere solicitud y aprobación).
  - Certificado de pago o Azure Trusted Signing (revisar si está disponible para particulares en España).
- **Después:** `winget` (manifiesto en `microsoft/winget-pkgs`) y quizá Microsoft Store.

---

## 8. Repositorio

- **Licencia: MIT.**
- README con GIF de demo, el porqué (generado frente a comprimido), instalación, el aviso de SmartScreen y cómo compilar.
- `CONTRIBUTING.md`, plantillas de issues (bug y propuesta) y GitHub Discussions activado para el feedback de la beta.
- **Cero telemetría.** Es un argumento de venta: dilo en el README.

---

## 9. Decisiones (cerradas el 2026-10-29)

| # | Decisión | Elegido |
|---|---|---|
| 1 | Nombre | **Rumor** |
| 2 | Clic izquierdo por defecto | **Alternar** |
| 3 | Atajo por defecto | **`Ctrl+Alt+P`** |
| 4 | Espectro en el panel | **Sí** |
| 5 | Idioma del repo | **README en inglés con sección en español**; código y commits en inglés |
| 6 | Windows 10 | **Soportado** (sin Mica, color sólido) |
