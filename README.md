# MCP papelería — prueba de vida

Un servidor MCP público, sin datos, para responder una sola pregunta: **¿Claude, en modo de voz
desde el teléfono, puede usar un conector nuestro y decir la respuesta en voz alta?**

Sale de la visión «la IA como interfaz» de `ro_inventario` (`docs/ia-como-interfaz.md`). No toca
ninguna base ni dato de la papelería. **Solo responde a quien entra por Cloudflare Access**: Access
hace el login (código por correo) y el OAuth del conector; el relevo valida el JWT que Access le
pasa en `Cf-Access-Jwt-Assertion` (`src/acceso.rs`).

## Herramientas

- `prueba_de_vida`: «Estoy vivo. Son las 10:42 en la papelería. Te respondo desde el cluster purépecha.»
- `mandar_mensaje(texto)` y `esperar_respuesta()`: el relevo hacia la sesión de Claude Code de la
  papelería (ver [el experimento](docs/control-remoto-por-voz.md)). Esperan hasta 20 s y devuelven
  la respuesta, una petición de permiso (se aprueba en la pantalla) o «sigue trabajando». Si el
  canal de la computadora no ha preguntado en el último minuto: «la computadora está apagada».

Del lado de la computadora, el canal pregunta por rutas JSON simples (no MCP), con el service token
de Access `canal-papeleria`: `GET /canal/mensajes` (espera hasta 25 s; cada mensaje trae `de`, el
correo de quien habló, y `texto`), `POST /canal/respuesta {texto}` y
`POST /canal/permiso {id, herramienta, descripcion}`. `/mcp` es solo para personas y `/canal`
solo para ese service token (403 al revés).

## Cómo corre

- Rust + [`rmcp`](https://crates.io/crates/rmcp) 3.5.1 (la misma que el MCP de imagina), Streamable
  HTTP en `/mcp`, sin sesiones y con respuestas JSON. `/healthz` para las sondas.
- Variables: `ACCESS_TEAM_DOMAIN` (el emisor, `https://negrita.cloudflareaccess.com`) y
  `ACCESS_AUD` (la «AUD tag» de la aplicación de Access) y `CANAL_CLIENT_ID` (el Client ID del
  service token del canal), obligatorias: sin ellas, o si no puede
  bajar las llaves de Access, no arranca. `PORT` (por omisión 8080) y `ALLOWED_HOSTS` (separados por coma). **rmcp rechaza
  cualquier `Host` que no esté en la lista** (403), para protegerse del DNS rebinding: en el
  cluster lleva `mcp-poc.xplaya.com`.
- CI publica `ghcr.io/rogithub/mcp_papeleria_poc` (amd64) en cada push a `main`. El paquete se
  marca público una vez, así el cluster no necesita credenciales.
- Se despliega con ArgoCD desde `k3s-manifests` (`workloads/mcp-papeleria-poc`) y sale a internet
  por el túnel de Cloudflare: `https://mcp-poc.xplaya.com/mcp`.

## Probar a mano

Sin un JWT de Access, `/mcp` responde 401 (y `/healthz`, 200):

```bash
PORT=5098 ACCESS_TEAM_DOMAIN=https://negrita.cloudflareaccess.com ACCESS_AUD=<aud> \
  CANAL_CLIENT_ID=<client-id> cargo run
curl -s -o /dev/null -w '%{http_code}\n' -X POST localhost:5098/mcp   # 401
```

Las herramientas se prueban por el conector, desde claude.ai.

## La prueba en el teléfono

1. En claude.ai (también desde el navegador del teléfono): Ajustes → Conectores → Agregar conector
   personalizado → `https://mcp-poc.xplaya.com/mcp`. Abre el login de Cloudflare: correo y el
   código que llega por correo. Solo entran los correos de la política de Access.
2. En la app del teléfono, modo de voz: «¿Está viva la papelería?» y «Repite: hoy vendimos cien
   copias». Lo que se busca es que **lo diga en voz alta**.

## El canal en la computadora de la papelería

`canal_papeleria` (`src/bin/canal_papeleria/`) es el MCP por stdio que arranca Claude Code: pregunta
al relevo por los mensajes del teléfono, los mete a la sesión como `<channel source="telefono"
de="correo">`, contesta con la herramienta `responder` y avisa al relevo las peticiones de permiso
(nunca las aprueba). CI lo publica para Linux x86 como `canal-papeleria` en el release
[`canal`](https://github.com/rogithub/MCP_papeleria_poc/releases/tag/canal).

- Variables: `RELEVO_URL` (`https://mcp-poc.xplaya.com`) y `CANAL_CLIENT_ID` (el Client ID del
  service token `canal-papeleria`).
- El Client Secret, en `~/.config/canal-papeleria/secreto` con permisos `600` (o en el archivo que
  diga `CANAL_SECRETO`): nunca en la configuración de Claude Code.
- **No se registra con `claude mcp add`**: así cualquier `claude` lo arrancaría, recogería los
  mensajes del teléfono y Claude Code los tiraría sin registrar el canal. Solo lo carga
  [`canal/claude-telefono`](canal/claude-telefono), que abre Claude Code con
  [`canal/telefono.mcp.json`](canal/telefono.mcp.json) (`--mcp-config`) y la bandera de canales.
  Ambos van en `~/.local/bin/` y `~/.config/canal-papeleria/` de `dayana`.
- Una regla *allow* para `mcp__telefono__responder` en la configuración de usuario (`/permissions`):
  si no, la pantalla pide aprobar cada respuesta.



[Control remoto por voz de Claude Code](docs/control-remoto-por-voz.md): hablarle desde el teléfono
a la sesión de Claude Code de la papelería, con OAuth de Cloudflare Access y un canal de Claude Code.
