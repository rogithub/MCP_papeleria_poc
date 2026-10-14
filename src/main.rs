//! Prueba de vida de un conector MCP público (ver README.md). No toca ninguna base ni dato de la
//! papelería: solo dice que está vivo y repite lo que le dicen. Solo responde a quien entró por
//! Cloudflare Access (`acceso`).

mod acceso;

use std::sync::Arc;

use axum::{Router, routing::get};
use chrono::Utc;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::schemars::{self, JsonSchema};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{tool, tool_router};
use serde::Deserialize;

/// La hora se dice en la de la papelería.
const ZONA: chrono_tz::Tz = chrono_tz::America::Cancun;

#[derive(Clone)]
struct Papeleria;

#[derive(Deserialize, JsonSchema)]
struct EcoParams {
    /// Lo que hay que repetir, tal cual.
    texto: String,
}

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
        description = "Repite el texto que se le da, tal cual. Sirve para probar que los \
                          parámetros llegan bien."
    )]
    async fn eco(&self, Parameters(EcoParams { texto }): Parameters<EcoParams>) -> String {
        format!("Me dijiste: «{texto}»")
    }
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

    // Sin las llaves de Access no tiene caso arrancar: todo /mcp respondería 401.
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
    let mcp = StreamableHttpService::new(
        || Ok(Papeleria),
        Arc::new(LocalSessionManager::default()),
        config,
    );

    let protected =
        Router::new()
            .nest_service("/mcp", mcp)
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
