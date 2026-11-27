# Fase W0: ¿suena Rumor con el móvil bloqueado?

Prueba de usar y tirar (`src/spike/`, `npm run build:spike`). Responde a una pregunta: si la app del móvil (PWA) puede seguir sonando con la pantalla bloqueada. Decide el camino de SPEC §10.3.

**Cómo se mide:** el reloj del `AudioContext` solo avanza mientras el navegador genera audio. La página lo compara con la hora real y apunta en el registro «el audio avanzó X de Y». **Audio vivo** es el total desde que le diste a sonar.

## Variantes
1. **AudioWorklet directo** (`ac.destination`), como la demo de marioyuste.com.
2. **AudioWorklet → `<audio>`** (`MediaStreamAudioDestinationNode`), con Media Session.
3. **La 2, más `navigator.audioSession.type = "playback"`.**

## Cómo probar cada variante
1. Abre la URL de vista previa. Para la prueba de app, instálala: en iPhone, **Compartir → Añadir a pantalla de inicio**; en Android, el menú → **Instalar app**.
2. Elige la variante (la página se recarga), dale a **Sonar** y sigue la lista.
3. Al acabar, **Copiar registro** y pégalo debajo.

| Prueba | Qué tiene que pasar |
|---|---|
| Bloquear 30 min | Se sigue oyendo; al desbloquear, el hueco dice ~100 % |
| Cambiar a otra app 2 min | Se sigue oyendo |
| Llamada entrante (o FaceTime) | Se corta durante la llamada; después, ¿vuelve? |
| Quitarse los AirPods | **Se para** (regla 10): «El sistema ha pausado el &lt;audio&gt;» |
| Interruptor de silencio (iPhone) | ¿Se sigue oyendo? |
| Pantalla de bloqueo | ¿Salen los controles de Rumor? ¿Funcionan? |

## Resultados

Marca: ✅ sí · ❌ no · ⚠️ a medias (explícalo).

### iPhone — modelo:  · iOS:

| Prueba | V1 navegador | V1 app | V2 navegador | V2 app | V3 navegador | V3 app |
|---|---|---|---|---|---|---|
| Bloquear 30 min (% audio vivo) | | ❌ se apaga al bloquear | | ✅ 15 min, 100 % cada minuto | | |
| Cambiar de app | | | | | | |
| Llamada entrante | | | | ⚠️ videollamada: se pausa; al volver a la app sale para reanudar, no vuelve solo | | |
| Quitarse los AirPods | | | | ✅ se para al instante y vuelve solo al ponérselos (tras el cambio) | | |
| Interruptor de silencio | | | | ✅ se sigue oyendo | | |
| Poner un vídeo en otra app | | | | ⚠️ se pausa; no vuelve solo al acabar | | |
| Controles en la pantalla de bloqueo | | | | ✅ pausa y play funcionan | | |

### iPad — modelo:  · iPadOS:

(Basta con la mejor variante del iPhone, instalada como app).

### Android — modelo:  · Android:  · Chrome:

| Prueba | V1 app | V2 app |
|---|---|---|
| Bloquear 30 min (% audio vivo) | | |
| Cambiar de app | | |
| Llamada entrante | | |
| Quitarse los cascos | | |
| Controles en la pantalla de bloqueo | | |

## Registros

**2026-11-27 · iPhone · variante 2 · app instalada.** Bloqueado de 19:40:15 a 19:46:43 (6,5 min) y **se oía todo el rato** (lo confirma Mario). La página no se congeló ni un segundo («Hueco de 0.0 s»). Pausa y play desde la pantalla de bloqueo funcionaron a las 19:44:45 y 19:44:47. Esta versión de la página aún no medía el audio minuto a minuto.

```
19:39:38  Lista · variante 2 · 48000 Hz · app instalada
19:40:00  ▶ Suena
19:40:00  <audio>: waiting
19:40:00  AudioContext: running
19:40:00  <audio>: suspend
19:40:00  <audio>: playing
19:40:15  Página oculta
19:44:45  ■ Parado
19:44:47  ▶ Suena
19:46:43  Página visible
19:46:43  Hueco de 0.0 s: el audio avanzó 0.0 s (0.0 %)
19:46:49  ■ Parado
```

**2026-11-27 · iPhone · variante 1 · app instalada.** Se apaga al bloquear el móvil. El truco del `<audio>` (variante 2) hace falta.

**2026-11-27 · iPhone · variante 2 · app instalada · quitarse los AirPods.** iOS manda una orden de «pausa» a la página (`Pantalla de bloqueo o cascos: pausa`) **después** de pasar el sonido al altavoz. La página la atendía haciendo el fundido de salida del motor (400 ms), y ese fundido se oía por el altavoz. Al ponérselos otra vez, no volvía a sonar. Tras la pausa, el `AudioContext` pasa a `suspended` y luego a `interrupted`.

```
20:07:40  ▶ Suena
20:07:41  Pantalla de bloqueo o cascos: pausa
20:07:41  ■ Parado
20:07:42  AudioContext: suspended
20:07:42  <audio>: pause
20:07:42  AudioContext: interrupted
```

**Cambio:** la página ya no registra órdenes propias de play y pausa (`setActionHandler`). Así es iOS quien pausa y reanuda el `<audio>` al instante, como con la música, y la página solo sigue al elemento para parar o arrancar el motor.

**Probado después del cambio:** al quitarse los AirPods deja de sonar, y al ponérselos **iOS lo reanuda solo**. La página sigue al `<audio>` en los dos sentidos.

```
20:14:00  <audio>: pause
20:14:00  El sistema ha pausado el <audio> → paro el motor
20:14:00  AudioContext: suspended
20:14:00  AudioContext: interrupted
20:14:04  <audio>: play
20:14:04  El sistema ha reanudado el <audio> → arranco el motor
20:14:04  <audio>: playing
20:14:04  AudioContext: running
```

**Para la app de verdad (W2):** no usar `setActionHandler` para play y pausa; dejar que iOS gobierne el `<audio>` y que el motor lo siga. En el iPhone, «seguir al volver los cascos» sale gratis; en Windows sigue siendo una opción, desactivada por defecto (SPEC §3.3).

**2026-11-27 · iPhone · variante 2 · app instalada · 15 min bloqueado.** De 20:25:51 a 20:41:03, el audio avanzó **el 100 % de cada minuto** (15 min 23 s en total). A las 20:41:07 iOS pausó el `<audio>` **porque Mario puso un vídeo en otra app**, y la página paró el motor. Al acabar el vídeo, **no vuelve solo**: el `AudioContext` pasa a `running`, pero el `<audio>` sigue en pausa. Es lo que hace iOS con la música. Si Rumor debe mezclarse con el vídeo o volver después, se decide en la W2 (ROADMAP v0.2, «Reacción a otro audio»).

```
20:25:45  ▶ Suena
20:25:51  Página oculta
20:26:46  Último tramo: el audio avanzó 60.9 s de 60.9 s (100.0 %)
   … (un tramo así cada minuto, todos al 100 %) …
20:40:48  Último tramo: el audio avanzó 60.0 s de 60.0 s (100.0 %)
20:41:03  Página visible
20:41:03  Último tramo: el audio avanzó 15.2 s de 15.2 s (100.0 %)
20:41:07  El sistema ha pausado el <audio> → paro el motor
20:41:07  Total hasta ahora: audio vivo 100.0 % de 15 min 23 s
```

## Conclusión

**iPhone (2026-11-27): superado con la variante 2.** La app web de Rumor, instalada en la pantalla de inicio, sigue sonando con la pantalla bloqueada si el audio sale por un elemento `<audio>` (AudioWorklet → `MediaStreamAudioDestinationNode` → `<audio>`).

- **Variante 1** (AudioWorklet directo): se apaga al bloquear. No vale.
- **Variante 2:**
  - 15 min bloqueado al 100 %,
  - controles en la pantalla de bloqueo,
  - AirPods fuera → se para al instante, y dentro → vuelve solo,
  - suena con el interruptor de silencio.
  - Un vídeo o una videollamada la pausan y hay que reanudarla, como pasa con la música.
- **Variante 3** (`audioSession = "playback"`): no hizo falta. La 2 ya suena con el interruptor de silencio.

**Para la app de verdad (W2):**
- Salida por `<audio>`, como en la variante 2.
- Media Session solo con `metadata`, sin `setActionHandler` para play y pausa: iOS gobierna el `<audio>` y el motor lo sigue.
- El botón de la página para el motor, espera el fundido de salida y después pausa el `<audio>`.

**Android:** sin probar. Se prueba en la W2, con la app ya hecha.
