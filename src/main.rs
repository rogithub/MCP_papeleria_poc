//! El relevo entre el Claude del teléfono y la sesión de Claude Code de la papelería (ver
//! docs/control-remoto-por-voz.md). No toca ninguna base ni dato de la papelería: pasa mensajes por
//! una fila en memoria (`fila`). Solo responde a quien entró por Cloudflare Access (`acceso`).

mod acceso;
mod canal;
mod fila;

use std::sync::Arc;
use std::time::Duration;

use axum::http::request::Parts;
use axum::{Router, routing::get};
use chrono::Utc;
use rmcp::ErrorData;
use rmcp::handler::server::common::Extension;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars::{self, JsonSchema};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{tool, tool_router};
use serde::Deserialize;

/// La hora se dice en la de la papelería.
const ZONA: chrono_tz::Tz = chrono_tz::America::Cancun;

/// Lo que el teléfono espera a la computadora en cada llamada. Lo que aguante el modo de voz
/// lo dice el experimento (pregunta 1 del documento).
const ESPERA: Duration = Duration::from_secs(20);

#[derive(Clone)]
struct Papeleria {
    fila: Arc<fila::Fila>,
}

#[derive(Deserialize, JsonSchema)]
struct MensajeParams {
    /// Lo que dijo la persona, con sus palabras.
    texto: String,
}

const APAGADA: &str = "La computadora de la papelería está apagada: no le llegó nada.";

#[tool_router(server_handler)]
impl Papeleria {
    #[tool(
        description = "Prueba de vida del sistema de la papelería: dice que está vivo, la hora en \
                       la papelería (Cancún) y desde dónde responde."
    )]
    async fn prueba_de_vida(&self) -> String {
        alive_message(Utc::now())
    }

    #[tool(
        description = "Le pasa un mensaje a la sesión de Claude Code abierta en la computadora \
                       de la papelería, la que tiene las impresoras, los pedidos y los archivos. \
                       Pasa lo que dijo la persona, con sus palabras, sin resolverlo tú ni \
                       agregarle nada. Espera hasta 20 segundos y devuelve lo que contestó la \
                       computadora: dilo en voz alta, corto. Si dice que sigue trabajando, usa \
                       esperar_respuesta. Si pide permiso, díselo a la persona: el permiso se da \
                       en la pantalla de la computadora, tú no puedes darlo."
    )]
    async fn mandar_mensaje(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(MensajeParams { texto }): Parameters<MensajeParams>,
    ) -> Result<String, ErrorData> {
        let Some(acceso::Identidad::Persona(de)) = parts.extensions.get().cloned() else {
            return Err(ErrorData::internal_error("no se sabe quién habla", None));
        };
        let inicio = std::time::Instant::now();
        tracing::info!(%de, %texto, "mensaje del teléfono");
        let contestacion = if self.fila.mandar(fila::Mensaje { de, texto }).await {
            fila::para_decir(&self.fila.esperar_salida(ESPERA).await)
        } else {
            APAGADA.into()
        };
        anotar("mandar_mensaje", inicio, &contestacion);
        Ok(contestacion)
    }

    #[tool(
        description = "Espera hasta 20 segundos a que la computadora de la papelería conteste lo \
                       último que se le mandó; no manda nada nuevo. Úsala cuando la respuesta \
                       diga que sigue trabajando. Lo que devuelva, dilo en voz alta, corto."
    )]
    async fn esperar_respuesta(&self) -> String {
        let inicio = std::time::Instant::now();
        let contestacion = if self.fila.prendida().await {
            fila::para_decir(&self.fila.esperar_salida(ESPERA).await)
        } else {
            APAGADA.into()
        };
        anotar("esperar_respuesta", inicio, &contestacion);
        contestacion
    }
}

/// Un log por llamada: qué se le contestó al teléfono y cuánto tardó. Con esto se mide en la
/// prueba cuánto espera el modo de voz (pregunta 1 del documento).
fn anotar(herramienta: &str, inicio: std::time::Instant, contestacion: &str) {
    tracing::info!(
        herramienta,
        ms = inicio.elapsed().as_millis() as u64,
        contestacion,
        "contestado al teléfono"
    );
}

fn required_env(name: &str) -> std::io::Result<String> {
    std::env::var(name)
        .map_err(|_| std::io::Error::other(format!("falta la variable de entorno {name}")))
}

fn alive_message(now: chrono::DateTime<Utc>) -> String {
    let hora = now.with_timezone(&ZONA).format("%H:%M");
    format!("Estoy vivo. Son las {hora} en la papelería. Te respondo desde el cluster purépecha.")
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    // rmcp solo acepta `Host: localhost` por omisión (contra DNS rebinding): el nombre público
    // tiene que estar en la lista, o toda petición desde afuera se rechaza.
    let allowed_hosts: Vec<String> = std::env::var("ALLOWED_HOSTS")
        .unwrap_or_else(|_| "localhost,127.0.0.1".into())
        .split(',')
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .collect();

    // Solo este service token entra a /canal: el Client ID no es secreto, el secreto lo tiene la PC.
    let canal_client_id: Arc<str> = required_env("CANAL_CLIENT_ID")?.into();

    // Sin las llaves de Access no tiene caso arrancar: todo respondería 401.
    let access = Arc::new(
        acceso::Access::new(
            &required_env("ACCESS_TEAM_DOMAIN")?,
            &required_env("ACCESS_AUD")?,
        )
        .await
        .map_err(std::io::Error::other)?,
    );

    // Sin sesiones y con respuestas JSON simples: nada de streams largos a través del túnel.
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_allowed_hosts(allowed_hosts.clone());
    let fila = Arc::new(fila::Fila::default());
    let mcp = StreamableHttpService::new(
        {
            let fila = fila.clone();
            move || Ok(Papeleria { fila: fila.clone() })
        },
        Arc::new(LocalSessionManager::default()),
        config,
    );

    let telefono = Router::new()
        .nest_service("/mcp", mcp)
        .layer(axum::middleware::from_fn(acceso::solo_personas));
    let canal = canal::rutas(fila).layer(axum::middleware::from_fn_with_state(
        canal_client_id,
        acceso::solo_el_canal,
    ));
    let protected = telefono
        .merge(canal)
        .layer(axum::middleware::from_fn_with_state(
            access,
            acceso::require_access,
        ));
    // /healthz queda fuera de Access: lo piden las sondas de k3s desde dentro del cluster.
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .merge(protected);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!(port, ?allowed_hosts, "escuchando");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn responde_que_esta_vivo_con_la_hora_de_cancun() {
        let now = Utc.with_ymd_and_hms(2026, 10, 14, 15, 42, 0).unwrap();

        assert_eq!(
            alive_message(now),
            "Estoy vivo. Son las 10:42 en la papelería. Te respondo desde el cluster purépecha."
        );
    }
}
