# Rumor — Especificación del producto

> Generador de ruido rosa, marrón y blanco **calculado en tiempo real** y **cronómetro de estudio** para Windows.
> Vive en la bandeja del sistema, se activa con un clic o un atajo y no reproduce ningún archivo de audio.
> Dos módulos (**Sonido** y **Estudio**) que funcionan juntos o por separado.

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
| Persistencia | `tauri-plugin-store` | Ajustes en JSON en `%APPDATA%` |
| Historial de estudio | **SQLite** (`rusqlite`, con `bundled`) | Sesiones en local; preparado para las estadísticas de v0.2 sin migrar datos |

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
- **Marrón:** integrador con fuga con el coeficiente **calculado según la frecuencia de muestreo** (codo en ~10 Hz), más un **filtro paso alto a ~20 Hz** para eliminar el DC y la deriva. (La versión con 0,02/1,02 fijos tenía el codo en ~150 Hz y no era marrón por debajo.)
- Funciona a la **frecuencia de muestreo nativa del dispositivo** (48 kHz normalmente). Nada de remuestrear.

### 3.2 Calidad (lo que lo hace "pulido")
- **Normalización de sonoridad entre colores:** a mismo volumen, rosa, marrón y blanco deben **sonar igual de fuertes**. Calibrar una ganancia por color con RMS, y como objetivo con ponderación K (tipo LUFS) a ±1 dB.
- **Sin clics nunca:**
  - Fundido de entrada y salida de unos **400 ms** (curva exponencial).
  - Cambios de volumen suavizados **muestra a muestra** (one-pole, unos 30 ms).
  - Cambio de color con **crossfade** de unos 150 ms (se generan ambos durante la transición).
- **Margen de seguridad:** pico ≤ −3 dBFS y un *soft clip* (tanh) como red de seguridad.
- **Volumen perceptual:** `gain = v²` o una curva en dB (−60 a 0 dB) sobre el slider del 0 al 100.

### 3.3 Robustez en Windows y cambio de salida de audio
- **Detección:** escuchar los cambios del dispositivo por defecto con `IMMNotificationClient` (crate `windows`, WASAPI), no solo esperar a que el stream de `cpal` falle. Leer el **tipo de salida** con `PKEY_AudioEndpoint_FormFactor` (cascos, headset o altavoces) para saber de dónde a dónde se ha cambiado.
- **Comportamiento al cambiar de salida** (ajuste *"Si se desconectan los cascos"*):

  | Opción | Qué hace |
  |---|---|
  | **Pausar si pasa a altavoces** *(por defecto)* | Igual que el móvil: quitas los cascos o se cae el Bluetooth → **se para** con un fundido rápido. Si cambias entre cascos, sigue |
  | Pausar siempre | Cualquier cambio de salida lo para |
  | Seguir sonando | Se reconecta a la nueva salida y continúa |

- Opción extra: **"Reanudar al volver a conectar los cascos"** (desactivada por defecto).
- La reconexión del stream al nuevo dispositivo se hace siempre (aunque esté pausado), para que el siguiente play funcione al instante.
- **Suspensión e hibernación:** al volver, reabrir el stream si murió y quedarse **en pausa**.
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

### 4.3 Atajos globales
- **`Ctrl + Alt + P`**: alternar el ruido. **`Ctrl + Alt + T`**: iniciar o pausar el cronómetro de estudio.
- Si un atajo está ocupado, avisar y proponer otro. Todos se configuran en los ajustes (capturando la combinación al pulsarla).
- **Ojo con AltGr:** en teclados españoles y de muchos países europeos, `Ctrl+Alt` equivale a **AltGr**. Los atajos por defecto **no pueden coincidir con un carácter** (`Ctrl+Alt+E` = €, `Ctrl+Alt+2` = @…). Validarlo al grabar un atajo nuevo y avisar.

### 4.3b Módulo Estudio
**Idea:** contar el tiempo que estudias **sin obligarte a ningún método**. Por defecto es un **cronómetro** que cuenta hacia arriba; el **Pomodoro** existe para quien lo quiera, pero no se impone.

**Modos** (se elige en el panel con un selector pequeño):

| Modo | Qué hace |
|---|---|
| **Cronómetro** *(por defecto)* | Cuenta hacia arriba. Play, pausa y terminar sesión. Sin avisos ni descansos |
| **Pomodoro** | Bloques de trabajo y descanso **configurables** (por defecto 25/5, y descanso largo de 15 cada 4). Aviso suave al cambiar de fase con notificación y, opcionalmente, un fundido del ruido durante el descanso |

**Lo que se ve en el panel** (tarjeta de Estudio):
- **Tiempo de la sesión actual** grande (`1:23:45`), con `tabular-nums`.
- **Hoy: 3 h 10 min** y **esta semana: 12 h 40 min**, en pequeño.
- Botones: play/pausa y terminar sesión.
- En Pomodoro: la fase actual ("Trabajo 2/4") y un anillo de progreso fino alrededor del tiempo.

**Juntar los dos módulos** (Ajustes → Estudio → *"Vincular con el ruido"*, **activado por defecto**):
- Al darle a play al ruido, **empieza a contar** el cronómetro; al pausar el ruido, **se pausa**. Un solo clic para "me pongo a estudiar".
- Si no está vinculado, cada módulo va por libre.
- En los descansos del Pomodoro, el ruido puede bajar o pararse (opcional).

**Detección de inactividad** (opcional, **desactivada por defecto**, porque si estudias con apuntes en papel no tocas el ordenador):
- Si no hay teclado ni ratón durante X minutos (por defecto 10; se lee con `GetLastInputInfo`), al volver pregunta: *"Has estado 14 min sin actividad. ¿Seguías estudiando?"* → **Sí, contar** / **No, descontar**.

**Al terminar una sesión**, se guarda en SQLite: inicio, fin, duración neta (sin pausas), modo y etiqueta.

**Etiquetas** (opcionales): asignar la sesión a una asignatura o tema ("Cálculo", "EDA"…), elegida de una lista que creas en Ajustes. Si no las usas, no aparecen en ninguna parte.

**Fuera de la beta** (v0.2): pantalla de **estadísticas** (gráfico semanal, por asignatura, racha de días) y **exportar a CSV**.

### 4.3c Módulos activables
- Ajustes → **Módulos**: ☑ Sonido · ☑ Estudio. Al menos uno tiene que estar activo.
- **Solo Sonido:** el panel queda exactamente como el del ruido; el cronómetro no existe.
- **Solo Estudio:** el panel enseña el cronómetro como protagonista; el clic izquierdo en la bandeja inicia o pausa el cronómetro.
- **Los dos:** el ruido arriba (protagonista) y la tarjeta de Estudio debajo, compacta.
- El **tooltip y el icono** se adaptan: `Rumor · Rosa 40 % · Estudiando 1:23:45`. El icono tiene una variante con un punto que indica que el cronómetro está corriendo.

### 4.4 Ajustes — sencillo por fuera, completo por dentro
El panel solo enseña lo que usas a diario. **Todo lo demás vive en Ajustes**, una ventana aparte (unos 560×640 px) con barra lateral de secciones. Cada opción tiene una línea de explicación debajo y un valor por defecto sensato: **nadie debería necesitar entrar para que la app funcione bien**.

| Sección | Opciones |
|---|---|
| **Sonido** | Color por defecto · ajuste fino de tono (v0.2) · duración del fundido (0,2–3 s) · volumen máximo permitido (el 100 % del slider equivale a ese máximo) |
| **Reproducción** | Si se desconectan los cascos (§3.3) · reanudar al reconectar · al iniciar Rumor: no sonar *(por defecto)* / sonar siempre / como lo dejé |
| **Temporizador** | Tiempos rápidos editables (por defecto 25/50/90) · fundido final largo (unos 10 s) · notificar al acabar |
| **Módulos** | Sonido · Estudio (§4.3c) |
| **Estudio** | Modo por defecto (cronómetro o Pomodoro) · tiempos del Pomodoro (trabajo, descanso, descanso largo, ciclos) · vincular con el ruido · qué hace el ruido en los descansos · detección de inactividad y minutos · etiquetas (crear, renombrar, colores) · inicio de la semana (lunes) |
| **Atajos** | Alternar ruido (`Ctrl+Alt+P`) · cronómetro (`Ctrl+Alt+T`) · subir y bajar volumen · siguiente color · abrir panel. Se graban pulsando la combinación |
| **Bandeja** | Acción del clic izquierdo · mostrar el tiempo restante en el tooltip |
| **Apariencia** | Tema: sistema, claro u oscuro · acento: el del ruido *(por defecto)* o el del sistema · efecto Mica · mostrar espectro · reducir animaciones |
| **Inicio** | Iniciar con Windows |
| **Avanzado** | Dispositivo de salida fijo (o "el predeterminado de Windows") · tamaño de buffer y latencia · info del dispositivo (frecuencia, canales) · **exportar/importar ajustes (JSON)** · abrir la carpeta de logs · restablecer todo |
| **Acerca de** | Versión · enlace a GitHub · licencia · cómo reportar un fallo |

- Los cambios se aplican **al instante**, sin botón de "Guardar".
- Decidido el 2026-10-29: el volumen máximo escala el slider; "empezar sonando" y "recordar el último estado" se funden en una sola opción; "empezar minimizado" se elimina (Rumor siempre arranca en la bandeja); el acento puede ser el del ruido o el del sistema. Las secciones **Módulos** y **Estudio** y el atajo del cronómetro llegan con la Fase 3b.
- Idioma: **Español / English**, detectado del sistema (también en Ajustes → Apariencia).

### 4.5 Diseño visual

**Objetivo:** que dé **calma solo con verlo**. Sencillo a primera vista, fluido al usarlo y sin nada que sobre. Referencia de *sensación* (no de copia): la interfaz de Claude, con colores cálidos, mucho aire, tipografía limpia, una sola cosa importante en pantalla y animaciones suaves que nunca molestan.

**Principios**
1. **Una cosa protagonista por pantalla.** En el panel es el botón de play; todo lo demás es secundario y más discreto.
2. **Divulgación progresiva.** Lo diario está en el panel; lo avanzado, en Ajustes. Nunca al revés.
3. **Calidez, no frialdad.** Neutros con un toque cálido (nunca gris puro) y **un único acento a la vez**, que sigue al ruido (rosa polvo, caramelo tostado o gris perla azulado). Identidad propia, no un clon de nadie.
4. **El movimiento informa.** Cada animación explica algo (se abre, cambia o suena); ninguna es decorativa sin motivo.
5. **Nativo de Windows.** Debe sentirse parte de Windows 11, no una web metida en una ventana.

**Tokens** (en `src/styles/tokens.css`, ajustables durante la Fase 3)

| Token | Claro | Oscuro | Uso |
|---|---|---|---|
| `--bg` | `#FAF7F5` | `#1C1A19` | Fondo (sobre Mica si está activo) |
| `--surface` | `#FFFFFF` | `#262322` | Tarjetas y controles |
| `--ink` | `#2B2523` | `#F2ECE9` | Texto principal |
| `--muted` | `#857A76` | `#A39793` | Texto secundario |
| `--line` | `#ECE4E0` | `#38322F` | Bordes de 1 px |
| `--accent-pink` | `#C7647F` | `#E8899F` | Acento con ruido rosa (rosa polvo) |
| `--accent-brown` | `#B5733D` | `#E5A05E` | Acento con ruido marrón (caramelo tostado) |
| `--accent-white` | `#76859A` | `#AFBACB` | Acento con ruido blanco (gris perla azulado) |
| `--accent-*-soft` | `#F7E1E6` · `#F3E4D6` · `#E6EAF0` | `#3D2A2F` · `#3B2E23` · `#2B3038` | Halos, relleno del espectro y fondos de selección |

**El acento sigue al ruido** (2026-10-29): `--accent` y `--accent-soft` toman el par del ruido seleccionado y cambian con una transición de 300 ms (`--dur-accent`), sincronizada con la curva del espectro. Solo cambia el acento (play, halo, curva, selección, slider, foco); el texto queda neutro y el icono de la bandeja, monocromo. Contraste: ~3,5:1 sobre el fondo en claro (mínimo WCAG para elementos gráficos, 3:1) y ≥ 7:1 en oscuro; el texto sobre los tonos suaves supera 11:1. El rosa claro pasa de `#D4728C` a `#C7647F` porque el original daba 2,99:1. El marrón oscuro es `#E5A05E` (más saturado, 7,9:1), decidido el 2026-10-29.

- **Tipografía:** **Segoe UI Variable** (la del sistema en Windows 11; pesa 0 KB y queda nativa), con `system-ui` como respaldo. Números con `tabular-nums` para que la cuenta atrás no baile.
- **Escala:** 12 / 14 / 16 / 20 / 28 px. Espaciado en múltiplos de 4 px. Radio de 12 px en tarjetas y 999 px en el botón de play.
- **Sombras:** casi ninguna. La separación se hace con espacio y bordes de 1 px.

**Movimiento**
- **Abrir panel:** fundido y desplazamiento de 8 px hacia arriba en **180 ms** (`cubic-bezier(.2,.8,.2,1)`). Cerrar, en 120 ms.
- **Play:** el botón se transforma en pausa con un morph del icono, y aparece un **halo que "respira" muy lento** (ciclo de 4 s) mientras suena.
- **Cambiar de color:** el espectro se transforma suavemente hacia la nueva curva en lugar de saltar.
- **Hover y pulsación:** 120 ms. Nada tarda más de 250 ms salvo el halo.
- **`prefers-reduced-motion`** o "reducir animaciones" → sin halo ni desplazamientos, solo fundidos.

**Estados que hay que diseñar** (no solo el "feliz"): sonando · en pausa · temporizador activo · cascos desconectados (aviso discreto en el panel: "Pausado: se desconectaron los cascos") · sin dispositivo de audio · atajo ocupado.

---

## 5. Alcance de la beta (v0.1.0-beta)

✅ **Entra**
- Motor Rust: rosa, marrón y blanco; estéreo decorrelado; normalización; fundidos; crossfade.
- Detección del cambio de salida con **pausa al quitar los cascos** (configurable, §3.3).
- Ventana de **Ajustes** completa (§4.4) y **sistema de diseño** de §4.5.
- **Módulo Estudio** (§4.3b): cronómetro, Pomodoro configurable, totales de hoy y de la semana, vinculación con el ruido, inactividad opcional, etiquetas y guardado en SQLite.
- **Módulos activables** (§4.3c).
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
- **Checklist manual** antes de cada release: `docs/QA.md` (quitar cascos con cable y Bluetooth con cada opción de §3.3, suspensión, atajo ocupado, tema claro y oscuro, escalado al 100/150/200 %, varios monitores, barra de tareas en otra posición).

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

## 9. Decisiones

| # | Decisión | Elegido | Estado |
|---|---|---|---|
| 1 | Nombre | **Rumor** | Cerrada (2026-10-29) |
| 2 | Clic izquierdo por defecto | **Alternar** | Cerrada (2026-10-29) |
| 3 | Atajo por defecto | **`Ctrl+Alt+P`** | Cerrada (2026-10-29) |
| 4 | Espectro en el panel | **Sí** | Cerrada (2026-10-29) |
| 5 | Idioma del repo | **README en inglés con sección en español**; código y commits en inglés | Cerrada (2026-10-29) |
| 6 | Windows 10 | **Soportado** (sin Mica, color sólido) | Cerrada (2026-10-29) |
| 7 | Al desconectar los cascos | **Pausar si pasa a altavoces** | Cerrada (2026-10-29) |
| 8 | Modo de estudio por defecto | **Cronómetro** | Cerrada (2026-10-29) |
| 9 | Vincular estudio con el ruido | **Sí** | Cerrada (2026-10-29) |
