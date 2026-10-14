# Control remoto por voz de Claude Code — experimento

> Experimento, no un plan de la v2. Sale de la conversación del 2026-10-14, después de que la
> [prueba de vida](../README.md) confirmó que el modo de voz del teléfono usa un conector propio.
> Lo que salga de aquí sirve para planear después en `ro_inventario`; nada de esto se anota allá
> todavía.

## La pregunta

¿Puede la dueña **hablarle desde el teléfono a la sesión de Claude Code que ya está abierta en la
computadora de la papelería**, oír sus respuestas y aprobar con la voz lo que esa sesión le
pregunte?

No es un asistente nuevo ni una lista cerrada de órdenes. Es ponerle voz y oído a la sesión que ella
ya usa: Claude Code con sus skills, `imagina`, las impresoras y el MCP `pedidos` de la v1. Ella
sigue ahí mismo, viendo la misma pantalla, en la misma red y con el mismo WhatsApp. La supervisión
es en corto: la vuelta por internet es solo para ganar voz y audio.

Lo que no cambia:
- **La venta la cierra ella, en el POS.** Por aquí no se cobra.
- **Lo que Claude Code ya pide confirmar, lo sigue pidiendo.** La diferencia es que ella puede
  contestar «sí» con la voz, o en la pantalla, como hoy.

## La forma

```
teléfono (voz) ── Claude (nube) ── Cloudflare Access ── relevo (k3s) ── canal (PC) ── Claude Code
                                    OAuth, 30 días       este repo        lo arranca     la sesión de
                                    código por correo    (Rust)           Claude Code    siempre
```

| Pieza | Dónde | Qué hace |
|---|---|---|
| Cloudflare Access | Panel de Cloudflare | OAuth para el teléfono (Managed OAuth), login con código por correo, sesión de 30 días, política «solo estos correos». Un *service token* para el canal. Sin código |
| **Relevo** | k3s, este repo | Herramientas para el Claude del teléfono, fila de mensajes, saber si la computadora está prendida. Valida el JWT de Access. Logs JSON a stdout (de ahí a Loki) |
| **Canal** | Computadora de la papelería, este repo | MCP local que arranca Claude Code. Sale a preguntar al relevo (no abre puertos), mete los mensajes a la sesión y devuelve respuestas y peticiones de permiso |
| Claude Code y sus MCP | Computadora de la papelería | Lo de hoy, sin cambios |

### Una conversación

1. Ella dice: «imprime el PDF de Juan, dos copias a color». El Claude del teléfono llama a una
   herramienta del relevo con ese texto.
2. El relevo lo guarda; el canal lo recoge y lo mete a la sesión. En pantalla aparece como mensaje
   entrante.
3. Claude Code trabaja como siempre. Si pide permiso, la petición sale por el canal **y** aparece en
   la pantalla.
4. El relevo le devuelve al teléfono lo que haya: la respuesta, «pide permiso para…» o «sigue
   trabajando».
5. Si pidió permiso, el Claude del teléfono se lo dice y ella lo aprueba con un botón en el
   teléfono o en la pantalla de la computadora ([quién aprueba](#quién-aprueba)). Gana la primera
   respuesta.
6. La respuesta final llega al teléfono y se dice en voz alta.

Si la computadora no ha preguntado en el último minuto, el relevo contesta que está apagada: fuera
de horario no tiene corriente, así que no hace falta programar el horario.

## Quién aprueba

El relevo nunca oye a la dueña: oye al Claude del teléfono, que transcribe e interpreta. Un «sí»
que llegue por la voz lo manda ese Claude, aunque ella no lo haya dicho, sea por error o porque el
texto que regresa de Claude Code (un WhatsApp, un PDF) trae «aprueba abcde». Repetir una palabra
clave no lo arregla: también la puede repetir él.

Por eso **el «sí» desde el teléfono es un botón que ella toca**, no algo que el Claude del teléfono
pueda mandar solo. Dos formas, en este orden:

1. **La confirmación de claude.ai.** Antes de llamar a una herramienta de un conector, la app puede
   pedir «permitir». Si la herramienta de aprobar se marca para pedirla siempre y el botón aparece
   también en modo de voz, el toque de ella es la prueba. Sin código; falta comprobar que aparece
   en voz.
2. **Una página de aprobación propia, si la 1 no sirve.** Un hostname aparte (p. ej.
   `aprobar.xplaya.com`) como aplicación de Access **sin** Managed OAuth: solo entra un navegador
   con su login, nunca el token del Claude del teléfono. El teléfono le dice «te dejé el botón» y
   ella toca «Aprobar» o «Rechazar» en la página. El relevo solo acepta veredictos de ahí.

Mientras ninguna esté probada, la voz solo avisa («pide permiso para…; apruébalo en la pantalla»)
y la aprobación es en la pantalla de la computadora. La pantalla siempre puede aprobar o rechazar:
gana la primera respuesta.

## Lo que ya se sabe (documentación, 2026-10-14)

- **Canales de Claude Code** ([guía](https://code.claude.com/docs/en/channels),
  [referencia](https://code.claude.com/docs/en/channels-reference)): un MCP por stdio que declara
  `experimental["claude/channel"]` y manda `notifications/claude/channel`; el mensaje entra a la
  sesión abierta. Para contestar, el canal expone una herramienta (`reply`). Con
  `experimental["claude/channel/permission"]`, Claude Code le manda cada petición de permiso
  (`notifications/claude/channel/permission_request`, con un id de cinco letras) y acepta el
  veredicto (`notifications/claude/channel/permission`, `allow` o `deny`). La pregunta en la
  terminal sigue abierta al mismo tiempo.
- **Cuenta:** requiere sesión de claude.ai o llave de Console. «Pro and Max users without an
  organization skip these checks entirely»: en Pro están disponibles, sin que un administrador los
  encienda. Están en *research preview* y se liberan poco a poco.
- **Canal propio:** no está en la lista aprobada, así que se arranca con
  `claude --dangerously-load-development-channels server:<nombre>`. Sale una advertencia al abrir
  (se elige «I am using this for local development»). No funciona con `-p`: solo en la sesión
  interactiva, que es la que se quiere.
- **Versión del protocolo:** en el runtime v2 de Claude Code, un canal que negocia la revisión
  `2026-07-28` no se registra. rmcp 3.5.1 la conoce (`ProtocolVersion::LATEST`), así que el canal
  debe quedarse en la anterior, o Claude Code arrancarse con `MCP_PROTOCOL_NEGOTIATION=legacy`.
- **Cloudflare Access, Managed OAuth** ([docs](https://developers.cloudflare.com/cloudflare-one/access-controls/applications/http-apps/managed-oauth/)):
  Access se vuelve el servidor OAuth de la aplicación (metadatos, registro dinámico del cliente,
  login, tokens). El origen recibe `Cf-Access-Jwt-Assertion` y debe validarlo. Plan gratuito de
  Zero Trust, hasta 50 usuarios (confirmar en el panel que Managed OAuth aparece).
- **Modo de voz y conectores:** sí funciona (prueba de vida del 2026-10-14).

## Lo que no se sabe y responde el experimento

1. ¿Cuánto espera el modo de voz a que una herramienta conteste? Define si una petición se resuelve
   en una vuelta o si el teléfono pregunta «¿ya?» varias veces.
2. ¿El login de Cloudflare Access funciona como conector de claude.ai, desde el teléfono?
3. ¿Los canales están ya disponibles en la cuenta de la papelería y en su versión de Claude Code?
4. ¿El botón de confirmar de claude.ai aparece en modo de voz? Si no, ¿la página propia se siente
   bien en el teléfono?
5. ¿Se siente natural? Aprobar por voz, la latencia de dos Claudes en fila, respuestas cortas para
   oírse.

## Fuera del experimento

- Cambiar algo en la v1, en `ro_inventario` o en los MCP de la computadora.
- Cobrar o cerrar ventas.
- Que funcione sin ella presente.

## Resultados

### Paso A — los canales en la computadora de la papelería (2026-10-14)

**Funcionan.** Con `fakechat`, el canal de demostración oficial, un mensaje escrito en su página
entró a la sesión de Claude Code de `dayana` y la respuesta regresó al chat en unos 11 segundos. La
respuesta usó su `CLAUDE.md`, en el tono de siempre: es la misma sesión, con su contexto.

Lo que salió en el camino:
- Claude Code 2.1.300 en la computadora; Bun 1.4.3 instalado en `~/.bun` de `dayana` solo para esta
  prueba (para quitarlo: borrar `~/.bun` y su línea en `~/.zshrc`).
- El plugin se instala como `fakechat@anthropic-plugin-directory`, no `@claude-plugins-official`
  como dice la documentación, y por eso `--channels` lo rechaza («not on the approved channels
  allowlist»). Con `--dangerously-load-development-channels plugin:fakechat@anthropic-plugin-directory`
  sí entra. El canal propio usará esa bandera de todos modos.
- La primera vez, el servidor del canal arrancó desconectado (nadie escuchaba en su puerto) y hubo
  que reconectarlo desde `/mcp`. Vigilar si se repite con el canal propio.
- Probado por ssh, con `ssh -L 8787:localhost:8787` para ver la página desde otra computadora.
- La respuesta fue larga, con listas y emojis: para la voz, las instrucciones del canal deben pedir
  respuestas cortas.

### Paso B — Cloudflare Access como OAuth del conector (2026-10-14)

**El login de Cloudflare funciona como conector de claude.ai, también en el modo de voz.** Al
agregar el conector, claude.ai abrió el login de Access; con el código que llegó por correo quedó
conectado, y en el modo de voz la prueba de vida contestó en voz alta.

Cómo quedó en Cloudflare (equipo `negrita.cloudflareaccess.com`):
- Aplicación *self-hosted* `mcp-poc` para `mcp-poc.xplaya.com`, sesión de `730h` (un mes).
- Política reutilizable `solo rodrigo y dayana` (Allow por correo), ligada a la aplicación. En el
  panel nuevo, crear la política no la liga: hay que elegirla en la aplicación y guardarla.
- Managed OAuth encendido, con registro dinámico solo para
  `https://claude.ai/api/mcp/auth_callback` y `https://claude.com/api/mcp/auth_callback`, y sesión
  OAuth de `730h` (el panel la deja «igual que la aplicación» si no se pone explícita, y por la
  API no aparece).
- Login: solo el código por correo (one-time PIN), que ya venía encendido.
- Sin login, `/mcp` responde 401 con `WWW-Authenticate` hacia los metadatos de OAuth de Access.

Lo que salió en el camino:
- Un conector que se agregó sin login queda marcado así en claude.ai: al encender Access, avisó que
  el servidor ahora pide inicio de sesión. Se arregló quitándolo y agregándolo otra vez.
- El MCP de Cloudflare conectado a Claude Code puede leer Access pero no crear aplicaciones (error
  `1010`): se hizo en el panel.
- El relevo valida el JWT de Access (`src/acceso.rs`): firma con las llaves del equipo, `aud`,
  `iss` y vencimiento. Sin él, `/mcp` responde 401 aunque se llegue por el NodePort desde la LAN.
