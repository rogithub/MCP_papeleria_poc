# MCP papelería — prueba de vida

Un servidor MCP público, sin datos, para responder una sola pregunta: **¿Claude, en modo de voz
desde el teléfono, puede usar un conector nuestro y decir la respuesta en voz alta?**

Sale de la visión «la IA como interfaz» de `ro_inventario` (`docs/ia-como-interfaz.md`). No toca
ninguna base ni dato de la papelería, y por eso va **sin autenticación**: el MCP de verdad lleva
OAuth. Al terminar la prueba, se apaga.

## Herramientas

- `prueba_de_vida`: «Estoy vivo. Son las 10:42 en la papelería. Te respondo desde el cluster purépecha.»
- `eco(texto)`: repite lo que se le dice; prueba que los parámetros llegan bien por voz.

## Cómo corre

- Rust + [`rmcp`](https://crates.io/crates/rmcp) 3.5.1 (la misma que el MCP de imagina), Streamable
  HTTP en `/mcp`, sin sesiones y con respuestas JSON. `/healthz` para las sondas.
- Variables: `PORT` (por omisión 8080) y `ALLOWED_HOSTS` (separados por coma). **rmcp rechaza
  cualquier `Host` que no esté en la lista** (403), para protegerse del DNS rebinding: en el
  cluster lleva `mcp-poc.xplaya.com`.
- CI publica `ghcr.io/rogithub/mcp_papeleria_poc` (amd64) en cada push a `main`. El paquete se
  marca público una vez, así el cluster no necesita credenciales.
- Se despliega con ArgoCD desde `k3s-manifests` (`workloads/mcp-papeleria-poc`) y sale a internet
  por el túnel de Cloudflare: `https://mcp-poc.xplaya.com/mcp`.

## Probar a mano

```bash
PORT=5098 ALLOWED_HOSTS=localhost cargo run
curl -s -H 'Accept: application/json, text/event-stream' -H 'Content-Type: application/json' \
  -H 'MCP-Protocol-Version: 2025-06-18' localhost:5098/mcp \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"prueba_de_vida","arguments":{}}}'
```

## La prueba en el teléfono

1. En claude.ai (también desde el navegador del teléfono): Ajustes → Conectores → Agregar conector
   personalizado → `https://mcp-poc.xplaya.com/mcp`.
2. En la app del teléfono, modo de voz: «¿Está viva la papelería?» y «Repite: hoy vendimos cien
   copias». Lo que se busca es que **lo diga en voz alta**.
