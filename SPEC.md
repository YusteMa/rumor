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
- **Suspensión e hibernación:** al volver, reabrir el stream si murió y quedarse **en pausa**. El cronómetro de estudio no cuenta el tiempo suspendido.
- **Bloqueo del PC** (Win+L), ajuste *"Al bloquear el PC"* en Reproducción:

  | Opción | Qué hace |
  |---|---|
  | **Pausar y reanudar si hay cascos** *(por defecto)* | Al bloquear se pausan el ruido y el cronómetro. Al desbloquear vuelven **solo si la salida sigue siendo de cascos**; si ahora son altavoces, se quedan en pausa (regla 10) |
  | Pausar | Se pausa al bloquear y no se reanuda solo |
  | No hacer nada | Sigue sonando con el PC bloqueado |

  Una pausa del usuario durante el bloqueo, o una reproducción ya parada antes de bloquear, nunca se "reanuda".
- **Volumen por salida:** Rumor recuerda un volumen para cada dispositivo (los cascos, los altavoces, la tele por HDMI…) y lo aplica al cambiar de salida, con la misma transición suave que el slider. Un dispositivo que no se ha usado antes empieza con el volumen actual. Activado por defecto; se puede desactivar en Ajustes → Sonido. El volumen máximo permitido sigue siendo uno para todos.
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

- **Icono con dos estados:** activo (con ondas) e inactivo (atenuado). Variantes para la barra clara y la oscura. Se dibuja en Rust al tamaño exacto del escalado.
- **Animación solo al cambiar de estado** (2026-10-31): al dar play las barras suben y al pausar bajan, en ~400 ms; mientras suena, el icono queda quieto para no distraer. Sin animación si Windows tiene desactivados los efectos de animación.
- **Icono de la app** (2026-10-31): tres barras redondeadas, una por ruido y en su acento oscuro (rosa, caramelo, perla), sobre una tesela cálida oscura (`#1C1A19`) con un borde de 1 px `#3A3432` para que no se funda con fondos oscuros. Lo genera `scripts/gen-app-icon.mjs`, cada tamaño en su rejilla de píxeles.
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
  4. Temporizador: **∞ / 25 / 50 / 90 min**, con cuenta atrás visible. Solo elegir un tiempo la reinicia; **pausar la congela y reproducir la continúa**. Cambiar de color o de volumen no la toca.
  5. Espectro en vivo (Rust envía unos 30 frames/s de bandas por evento; **solo con el panel visible**).
  6. Pie: ⚙ Ajustes · Salir.
- **Accesible:** navegable con teclado, foco visible, `Espacio` alterna y `Esc` cierra.

### 4.3 Atajos globales
- **`Ctrl + Alt + P`**: alternar el ruido. **`Ctrl + Alt + T`**: iniciar o pausar el cronómetro de estudio.
- Si un atajo está ocupado, avisar y proponer otro. Todos se configuran en los ajustes (capturando la combinación al pulsarla).
- **Ojo con AltGr:** en teclados españoles y de muchos países europeos, `Ctrl+Alt` equivale a **AltGr**. Los atajos por defecto **no pueden coincidir con un carácter** (`Ctrl+Alt+E` = €, `Ctrl+Alt+2` = @…). Validarlo al grabar un atajo nuevo y avisar.

### 4.3b Módulo Estudio
**Idea:** contar el tiempo que estudias **sin obligarte a ningún método**. Es un **"Modo estudio" opcional, desactivado por defecto** (§4.3c): Rumor es solo ruido hasta que lo activas. Por defecto es un **cronómetro** que cuenta hacia arriba; el **Pomodoro** existe para quien lo quiera, pero no se impone.

**Modos** (se elige en el panel con un selector pequeño):

| Modo | Qué hace |
|---|---|
| **Cronómetro** *(por defecto)* | Cuenta hacia arriba. Play, pausa y terminar sesión. Sin avisos ni descansos |
| **Pomodoro** | Bloques de trabajo y descanso **configurables** (por defecto 25/5, y descanso largo de 15 cada 4). Cambia de fase solo, con el aviso elegido. En los descansos el ruido se **baja a la mitad** *(por defecto)*, se **pausa** o **sigue igual** (un solo desplegable), siempre con fundido |

**Lo que se ve en el panel** (tarjeta de Estudio):
- **Tiempo de la sesión actual** grande (`1:23:45`), con `tabular-nums`.
- **Hoy: 3 h 10 min** y **esta semana: 12 h 40 min**, en pequeño.
- Botones: play/pausa y terminar sesión.
- En Pomodoro: la fase actual ("Trabajo 2/4") y un anillo de progreso fino alrededor del tiempo.

**Juntar los dos módulos** (solo con el Modo estudio activado; *"Vincular con el ruido"*, **activado por defecto**), **en los dos sentidos**:
- Play en el ruido arranca el cronómetro y play en el cronómetro arranca el ruido; igual con la pausa. Un solo clic para "me pongo a estudiar".
- Las pausas automáticas del ruido (cascos desconectados, bloqueo del PC, suspensión) también pausan el cronómetro. Los descansos del Pomodoro no: los gobierna el Pomodoro.
- Si no está vinculado, cada módulo va por libre.

**Lo que cuenta:** solo el **tiempo de trabajo**. Pausas, descansos del Pomodoro, el PC bloqueado o suspendido no suman a la sesión ni a los totales de hoy y de la semana.

**Detección de inactividad** (opcional, **desactivada por defecto**, porque si estudias con apuntes en papel no tocas el ordenador):
- Si no hay teclado ni ratón durante X minutos (por defecto 10; se lee con `GetLastInputInfo`), al volver se plantea la pregunta *"Has estado 14 min sin actividad. ¿Seguías estudiando?"* → **Sí, contar** / **No, descontar**, **sin depender de las notificaciones ni interrumpir**:
  - El icono de la bandeja muestra un puntito y el tooltip dice *"¿Seguías estudiando?"* mientras la pregunta esté pendiente.
  - La pregunta espera en el panel hasta que lo abras. Rumor **nunca abre el panel solo ni quita el foco** a lo que estés haciendo.
  - Si las notificaciones están activadas, además se manda una discreta.
  - Si no contestas antes de terminar la sesión, el tiempo cuenta.

**Al terminar una sesión**, se guarda en SQLite: inicio, fin, duración neta (sin pausas), modo, etiqueta el ruido con el que empezó y sus tramos de trabajo. **Una sesión cuenta entera en el día en que empezó**, aunque pase de la medianoche (§4.3d). Una sesión en curso se guarda cada minuto, así que un cierre inesperado pierde como mucho un minuto. Al salir de Rumor la sesión se termina y se guarda. **Hoy** y **esta semana** se cuentan en hora local; la semana empieza el día que diga la configuración regional de Windows.

**Etiquetas** (opcionales): asignar la sesión a una asignatura o tema ("Cálculo", "EDA"…), elegida de una lista que creas en Ajustes. Si no las usas, no aparecen en ninguna parte.

**Avisos:** un **único ajuste para toda la app**, *"Avisos: Campana / Notificación / Las dos / Nada"*, que vale para el fin del temporizador de apagado, los cambios de fase del Pomodoro y el descanso de oídos. La **campana** es un **cuenco tibetano**: un *"bong"* suave con cola larga, **sintetizado en el momento** (parciales inarmónicos con caída exponencial, sin archivos de audio), a un volumen moderado que se mezcla encima del ruido sin cortarlo. Suena aunque el ruido esté en pausa.

**Descanso de oídos** (activado por defecto, desactivable): tras **X horas escuchando con cascos** (por defecto **3 h**, configurable) Rumor recuerda descansar con el aviso elegido. Cuenta solo el tiempo sonando por una salida de cascos; una pausa de **15 minutos o más** reinicia la cuenta. No para el ruido: solo avisa, una vez por tramo.

**Estadísticas:** en su propia ventana (§4.3d).

### 4.3c Módulos activables
- **Por defecto Rumor es solo ruido.** El cronómetro es un **"Modo estudio" opcional, desactivado por defecto**. Con él desactivado, **nada** del cronómetro aparece en el panel, la bandeja, el tooltip ni los atajos.
- Ajustes → **Estudio → Modo estudio** (sí/no) sustituye a la antigua sección "Módulos" (decidido el 2026-10-30): el ruido está siempre; el estudio se añade.
- **Desactivado:** el panel queda exactamente como el del ruido; el cronómetro no existe (ni su atajo ni su punto en la bandeja ni su tooltip).
- **Activado:** el ruido arriba (protagonista) y la tarjeta de Estudio debajo, compacta; el panel crece lo justo para ella.
- El **tooltip** se adapta: `Rumor · Rosa 40 % · Estudiando 1:23:45`. El icono lleva un punto **solo** mientras espera la pregunta «¿Seguías estudiando?» (§4.3b); con el cronómetro corriendo sin más, no hay punto (decidido el 2026-11-03).

### 4.3d Estadísticas de estudio
*Aprobada el 2026-11-05 con los ajustes de la segunda ronda de capturas (`docs/fotos/v0.2-estadisticas-entrega-1/ronda-2`) y los retoques de la tercera (`ronda-3`). Referencia visual: `docs/design/stats-mockup.html`.*

**Idea:** ver de un vistazo cuánto, cuándo y en qué estudias, con el mismo acabado que el resto de Rumor y **sin castigar el descanso**. Solo existe con el Modo estudio activado (§4.3c).

**Dónde:** una **ventana propia, "Estadísticas"**, como Ajustes: barra de título de Windows, Mica, una sola instancia, 820×720 px al abrirse (mínimo 560×480, siempre dentro de la pantalla), tema claro y oscuro. Se abre con un botón pequeño en la línea de totales de la tarjeta de Estudio del panel (*"Hoy 3 h 10 min · Semana 12 h 40 min"* y, a la derecha, un icono de barras con *"Estadísticas"*); el panel no cambia de alto y se cierra al abrirla, como con Ajustes.

**Cabecera:** título "Estadísticas", el periodo (*"27 oct – 2 nov"*) con flechas ‹ › para ir a periodos anteriores (› se apaga en el actual) y un segmentado con las pestañas **Semana** *(por defecto)* · **Mes** · **Año** · **Sesiones**. La ventana recuerda la última pestaña.

**Semana**
- **Total de la semana** grande, con la **comparación con la semana anterior** en una píldora (*"+18 % vs semana pasada"*: verde si sube, neutra si baja; nunca roja). En la semana en curso se compara **a la misma altura** (hasta el mismo día y hora de la anterior) y se dice al lado: *"hasta el jueves 20:44"*. Si la anterior, hasta ese punto, no llega a **30 min**, un porcentaje no significa nada: se da la diferencia en minutos (*"+25 min"*).
- Junto al total, el **anillo del objetivo semanal** (opcional, ver *Objetivo y racha*), con el porcentaje dentro.
- Cuatro cifras: **media diaria** (entre los días transcurridos de la semana), **sesión media**, y dos más según la entrega: *sesiones* y *días con estudio* en la primera; **racha** y **ruido favorito** en la segunda.
- **Horas por día:** barras verticales **apiladas por etiqueta**, en el orden de la leyenda, con:
  - **puntas redondeadas** arriba y base recta sobre el eje;
  - **2 px de separación** entre los tramos de cada etiqueta;
  - un **brillo lateral sutil** (gradiente horizontal: luz a la izquierda, un poco de sombra a la derecha) y una **sombra suave en la base**;
  - **línea discontinua del objetivo diario** (objetivo semanal ÷ 7) con su rótulo, solo si hay objetivo;
  - el día de hoy con la letra en negrita, y la cuadrícula en horas redondas con una **escala fija de al menos 6 h**, igual en todas las semanas para que las barras se puedan comparar al pasar de una a otra; solo crece si una semana la supera;
  - **al pasar el ratón** (o con el teclado: cada barra es enfocable), las demás barras se atenúan y sale una etiqueta flotante con el día, su total y el reparto por etiqueta. Con movimiento reducido, sin transiciones.
- **Por asignatura:** barras horizontales finas con nombre, tiempo y porcentaje, de más a menos; "Sin etiqueta" siempre al final. Si no usas etiquetas, esta tarjeta no aparece y las barras por día van en el color del acento.
- **Cuándo estudias más:** mapa de calor **hora × día** de las **últimas 8 semanas** hasta la mostrada (filas: días de la semana; columnas: de 6 a 23 h, una por hora, y una columna aparte **"noche"** para lo que cae entre las 23 y las 6 h), con 5 niveles de intensidad del acento, y debajo **"Tu mejor franja: 10 – 12 h"** (las 2 horas seguidas con más tiempo entre las 6 y las 23 h; la noche no es franja). Cada sesión va en la fila del día en que empezó, y su trabajo en las horas en que pasó de verdad. Sin al menos 5 sesiones y 5 h en esas semanas, la tarjeta no aparece.
- **Récords** (de todo el historial, sesiones terminadas): sesión más larga, mejor día, mejor semana y total histórico, cada uno con su fecha.
- **Así ha ido:** hasta 3 **frases automáticas**: las que **más se desvían de la media del propio usuario**, ordenadas por cuánto se desvían, con **un solo ▼ como mucho**: si hay candidatas neutras o positivas, van antes que un segundo descenso. Nunca repiten lo que ya está en pantalla (la píldora del %, el récord de sesión más larga, los días con estudio) y nunca salen con pocos datos (la semana necesita al menos 2 sesiones y 1 h). En la semana en curso todo se compara a la misma altura. Las candidatas:
  - una asignatura que baja o sube **dos semanas seguidas**, un 25 % o más cada vez, partiendo de al menos 1 h (*"Redes lleva dos semanas bajando"*). Solo con **semanas cerradas**: en la semana en curso se miran las tres anteriores, nunca la mitad que va de esta;
  - la semana frente a **tu semana media** de las 8 anteriores con estudio (al menos 4), si se aparta un 20 % o más. Como la píldora compara con la semana pasada, la frase dice su base: *"Llevas un 30 % más que tu media de las 6 semanas anteriores a estas alturas"* (en una semana pasada, *"Estudiaste un 30 % más que…"*);
  - la **duración de las sesiones** frente a la habitual (al menos 10 sesiones antes y 3 esta semana; un 20 % y 5 min de diferencia);
  - **el momento del día**: más por la mañana (< 14 h), por la tarde (< 20 h) o por la noche que de costumbre (25 puntos de diferencia);
  - **tu mejor día de la semana**, con al menos 4 semanas de historial (*"Los martes son tu mejor día"*), con menos peso porque cambia poco;
  - ruido (segunda entrega): *"Con ruido marrón tus sesiones duran un 20 % más que con blanco"*, solo con al menos 5 sesiones de cada color comparado y una diferencia de al menos un 15 %.
  - El tono siempre es positivo o neutro: un descenso se cuenta como dato, sin reproche. Marcadores: ▲ verde para lo que sube, ▼ caramelo para lo que baja y ● gris neutro para lo informativo; nunca rojo.

**Mes**
- **Calendario** del mes (semanas en filas y días en columnas, empezando el día que diga Windows), cada día **coloreado por intensidad** (5 niveles del acento, relativos al mejor día del mes) con su número; hoy, con contorno. Al pasar el ratón: fecha y total.
- **Total del mes** con **comparación con el mes anterior** (a la misma altura en el mes en curso: el mismo día, o el último si el mes anterior es más corto, y la misma hora; con el mismo umbral de 30 min), días con estudio (*"24 de 31"*, o de los días transcurridos en el mes en curso), media por día de estudio y mejor día.
- **Por asignatura** del mes, igual que en Semana.

**Año** *(segunda entrega)*
- **Mapa de cuadraditos tipo GitHub**: 53 columnas de semanas × 7 días, 5 niveles del acento y los meses rotulados arriba.
- **Evolución mensual**: 12 barras con el mismo estilo que las de Semana (sin apilar), con el total y la media del año.

**Sesiones**
- **Lista por días** (los más recientes arriba, con el total de cada día en su cabecera). Cada sesión muestra la hora de inicio y fin, la duración, la etiqueta (punto + nombre), el color de ruido si se guardó y una marca discreta "editada" o "añadida a mano".
- **Editar** una sesión, en la misma fila: corregir la **duración** o la **etiqueta**. El campo de duración lleva el formato a la vista (**"h:mm"** junto a la etiqueta y como texto de ayuda) y acepta también minutos sueltos (*"90"* = 1:30). Una sesión son sus **tramos de trabajo** (lo que hay entre pausas; sin pausas, un solo tramo del inicio al fin). Si acortas, se quitan minutos desde el final: primero del último tramo y, si no basta, del anterior. Si alargas, crece el último tramo, así que **el inicio nunca se mueve y solo cambia el fin**. Se valida igual que al añadir: no puede solaparse con otra sesión ni acabar en el futuro. El aviso de solape dice **con cuál**: *"Se solapa con la sesión de las 18:20"* (con el día delante si es de otro).
- **Borrar**, con "Deshacer" durante unos segundos en lugar de pedir confirmación (la sesión solo se marca; desaparece del todo al volver a abrir Rumor).
- **Añadir a mano** (*"+ Añadir sesión"*): fecha, hora de inicio, duración y etiqueta. No puede solaparse con otra sesión ni quedar en el futuro.
- **Exportar a CSV** (botón en esta pestaña): **todo el historial**, no solo lo visible, una fila por sesión con fecha, inicio y fin (hora local), duración (h:mm:ss), segundos, etiqueta, ruido, modo, si está editada o añadida a mano y si está en curso. Separador ",", UTF-8 con BOM y saltos CRLF, para que Excel en español lo abra bien.
- La sesión en curso aparece arriba como *"19:58 – ahora"* con la marca "En curso", y **no se puede editar ni borrar** hasta que termine.
- La lista se carga **por días** (14 cada vez, con "Ver más"), nunca el historial entero.

**Objetivo y racha** *(segunda entrega)*
- **Objetivo semanal** opcional (Ajustes → Estudio, desactivado por defecto; de 1 a 80 h). Sin objetivo no hay anillo, ni línea de objetivo diario, ni racha.
- **Racha por semanas, nunca por días:** cuántas semanas seguidas se ha cumplido el objetivo. La semana en curso suma en cuanto se cumple y no rompe la racha mientras no acabe. Descansar un día no castiga nada.
- **Ruido favorito:** el color con más tiempo de estudio en las últimas 4 semanas.

**Estados vacíos** (cuidados, nada de un simple "Sin datos"):
- **Sin ninguna sesión:** las barras de la semana dibujadas en fantasma (solo contorno) y el texto *"Aquí verás tus horas de estudio. Pulsa play en la tarjeta de Estudio del panel para empezar tu primera sesión."*
- **Semana o mes sin estudio, pero con historial:** las mismas barras en fantasma con *"Esta semana aún no has estudiado"* y *"Descansar también cuenta"*, un enlace a la última semana con datos y los récords visibles.
- Las pestañas Mes y Sesiones nunca parecen deshabilitadas; en Sesiones vacía está **"+ Añadir sesión"**, para cargar sesiones pasadas.
- **Pocos datos:** las tarjetas que los necesitan (mejor franja, frases) se ocultan en vez de mostrar cifras sin sentido.

**Datos y rendimiento**
- **Una sola agregación** (`study/stats.rs`, pura y testeada) alimenta el "Hoy" y "Semana" del panel, la ventana de Estadísticas y el CSV, así que siempre coinciden con la lista de sesiones. Editar, añadir, borrar o deshacer actualiza al momento todo lo que depende de esa sesión: semana, mes, panel, récords, mapa de calor y frases.
- **La sesión en curso, una sola regla** (decidido el 2026-11-05, tercera ronda): **suma como tiempo** en todo lo que es tiempo (el total, las barras, las asignaturas, la media diaria, la comparación, el mes y el "Hoy" del panel), así que el panel, la ventana y el CSV cuadran al segundo con lo que lleva trabajado. **No cuenta como sesión hasta que termina**: queda fuera del número de sesiones, la sesión media, los récords (incluido el mejor día del mes), las frases y "Cuándo estudias más".
- **Una sesión cuenta entera en el día local en que empezó**, aunque cruce la medianoche (decidido el 2026-11-05). Las fechas se guardan en UTC (milisegundos) y se agrupan en la hora local del sistema (Europe/Madrid en España): los días de cambio de hora tienen 23 o 25 horas y no se pierde nada.
- **"A la misma altura"**: las sesiones de la semana (o el mes) anterior que empezaron antes de ese punto, con el trabajo que llevaban hecho en ese instante.
- Migración 2 (reversible, `up` y `down`; **antes de migrar un historial existente se copia el archivo** a `study.sqlite3.v1.bak`): `sessions.noise`, `sessions.edited` (0 = no, 1 = editada, 2 = añadida a mano), `sessions.deleted_at` (borrado que aún se puede deshacer) e índice `sessions_by_start`. Las sesiones antiguas quedan con `noise = NULL`.
- **El ruido de una sesión es el del inicio** (decidido el 2026-11-05): el color que suena al empezar o, si está vinculada con el ruido, el que va a sonar con ella; `NULL` si se estudia en silencio. No se actualiza si cambias de color a mitad: es lo que eligió para ponerse a estudiar. **Se empieza a guardar en la primera entrega**, para que en la segunda ya haya datos para las frases.
- La ventana pide cada vista con un comando (`stats_week(offset)`, `stats_month(offset)`, `stats_sessions(before, days)`, `session_update`, `session_add`, `session_delete`, `session_restore`, `stats_export_csv`) y escucha `stats-changed` (al terminar, guardar cada minuto, editar, borrar o añadir una sesión) y `stats-context` (idioma, acento o Mica). Como siempre, el estado vive en Rust (regla 3).
- **Gráficas en SVG propio**, sin librerías. Todos los colores, radios, separaciones, opacidades y duraciones salen de `tokens.css` (tokens nuevos `--chart-*`, y `--tag-none` para "Sin etiqueta"); las etiquetas usan sus `--tag-*`, ya armonizados con el acento.
- Accesible: cada gráfica tiene un resumen en texto para lectores de pantalla y se puede recorrer con el teclado. **Contraste ≥ 4,5:1 en todo el texto** (decidido el 2026-11-05): el gris secundario de la app pasa a `#736863` en claro; los números del calendario van sobre niveles de intensidad que lo cumplen en claro y oscuro, y "Sin etiqueta" se distingue del fondo en los dos temas (≥ 3:1, como gráfico: `#988c87` en claro). La pestaña elegida lleva, además del fondo, una **raya corta del acento** (≥ 3:1 en claro y oscuro), porque el fondo solo no llega.

**Entregas**
1. **Primera:** la ventana, Semana (sin anillo, línea de objetivo, racha ni ruido favorito), Mes, Sesiones (editar, borrar, añadir y exportar a CSV), frases y estados vacíos; se empieza a guardar el ruido de cada sesión. La pestaña Año aún no aparece.
2. **Segunda:** Año, objetivo semanal con anillo y línea, racha por semanas, ruido favorito y frases sobre el ruido.

### 4.4 Ajustes — sencillo por fuera, completo por dentro
El panel solo enseña lo que usas a diario. **Todo lo demás vive en Ajustes**, una ventana aparte (720×640 px al abrirse, como mínimo 520×440, y siempre dentro de la pantalla) con barra lateral de secciones. Cada opción tiene una línea de explicación debajo y un valor por defecto sensato: **nadie debería necesitar entrar para que la app funcione bien**.

| Sección | Opciones |
|---|---|
| **General** | Si se desconectan los cascos (§3.3): pausar *(por defecto)* / pausar y reanudar al reconectarlos / pausar con cualquier cambio de salida / seguir sonando · al bloquear el PC (§3.3) · **avisos**: campana / notificación / las dos *(por defecto)* / nada · **descanso de oídos**: desactivado / cada 2, 3 *(por defecto)* o 4 h · **modo mono** · al iniciar Rumor: no sonar *(por defecto)* / sonar siempre / como lo dejé · iniciar con Windows · reacción a otro audio (v0.2) |
| **Estudio** | **Modo estudio** (sí/no, desactivado por defecto). Solo con él activado: tiempos del Pomodoro (trabajo, descanso, descanso largo y bloques) · ruido en los descansos · vincular con el ruido · inactividad: desactivada / 10 / 15 / 20 / 30 min · etiquetas (crear, renombrar, colores) |
| **Atajos** | Alternar ruido (`Ctrl+Alt+P`) · subir y bajar volumen · siguiente color · abrir panel · cronómetro (`Ctrl+Alt+T`, solo con el Modo estudio) · acción del clic izquierdo en el icono. Se graban pulsando la combinación |
| **Apariencia** | Tema: sistema, claro u oscuro · acento: el del ruido *(por defecto)* o el del sistema · mostrar espectro · idioma |
| **Avanzado** | *Sonido:* color al iniciar · duración del fundido (0,2–3 s) · volumen máximo (el 100 % del slider equivale a ese máximo) · volumen de cada salida (§3.3) · ajuste fino de tono (v0.2). *Temporizador:* tiempos rápidos (25/50/90) · fundido final (unos 10 s). *Salida de audio:* dispositivo fijo · buffer · salida actual. *Ventana:* efecto Mica. *Mantenimiento:* exportar/importar ajustes (JSON) · registros · restablecer todo |
| **Acerca de** | Versión · enlace a GitHub · licencia · cómo reportar un fallo · **actualizaciones**: buscar (activado por defecto) y "Buscar ahora" / "Actualizar" (§7) |

- Los cambios se aplican **al instante**, sin botón de "Guardar".
- Decidido el 2026-10-29: el volumen máximo escala el slider; "empezar sonando" y "recordar el último estado" se funden en una sola opción; "empezar minimizado" se elimina (Rumor siempre arranca en la bandeja); el acento puede ser el del ruido o el del sistema.
- Decidido el 2026-10-30 (simplificación de Ajustes): de 11 secciones a 6. Se quitan "reducir animaciones" (manda el ajuste "Efectos de animación" de Windows), "tiempo restante en el tooltip" (se muestra siempre), "modo por defecto" del estudio (el panel recuerda el último) e "inicio de la semana" (se toma de la configuración regional de Windows: lunes en España). "Reanudar al reconectar" se junta con "si se desconectan los cascos"; "avisar al acabar" pasa al ajuste único de avisos.
- El **descanso de oídos** va en General y no en Estudio: protege los oídos de cualquiera que use cascos, tenga o no el Modo estudio activado.
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
- **Módulo Estudio** (§4.3b): cronómetro, Pomodoro configurable, totales de hoy y de la semana, vinculación con el ruido, inactividad opcional, etiquetas, avisos configurables, descanso de oídos y guardado en SQLite.
- Volumen por salida, pausa al bloquear el PC y modo mono (§3.3, §4.4).
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
- Firma de código, controles multimedia, ecualizador de tono, mezclador de sonidos, macOS y Linux.

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
- **Auto-actualización** (desde la 0.1.0-beta.2, decidido el 2026-11-05), con `tauri-plugin-updater`:
  - Busca versión nueva **30 s después de abrirse y cada 6 h**, activado por defecto (Ajustes → Acerca de lo desactiva). Es la única petición de red de Rumor y no envía nada del usuario.
  - Al encontrarla: **una notificación por versión** y, en el pie del panel, el botón **"Actualizar"** en lugar del texto del atajo (el panel no cambia de alto). También en Ajustes → Acerca de.
  - Un clic: descarga sin cortar el ruido, guarda la sesión de estudio y los ajustes, **funde el ruido**, instala en modo pasivo y **Rumor se vuelve a abrir solo**; si sonaba, sigue sonando, pero solo si la salida es la misma que antes (si se desenchufan los cascos durante la actualización, se queda en pausa: §3.3).
  - Los instaladores van **firmados con la clave del updater** (la privada en los secretos del repo, la pública en `tauri.conf.json`); sin firma válida no se instala nada.
  - El manifiesto `latest.json` vive en la rama `updater` (lo escribe el workflow de release), porque el enlace "latest release" de GitHub se salta las pre-releases.
  - Las compilaciones de desarrollo no buscan solas; solo con "Buscar ahora".

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
| 9 | Modo estudio y vinculación | **Modo estudio desactivado por defecto**; al activarlo, vinculado con el ruido en los dos sentidos | Cerrada (2026-10-30) |
| 10 | Icono de la app | **Tres barras de acento sobre tesela oscura con borde fino** | Cerrada (2026-10-31) |
| 11 | Icono de la bandeja | **Animado solo al cambiar de estado** (~400 ms), quieto mientras suena | Cerrada (2026-10-31) |
| 12 | Auto-actualización | **Activada por defecto**, aviso en el panel + un clic (§7) | Cerrada (2026-11-05) |
