//! El canal de Claude Code en la computadora de la papelería (ver docs/control-remoto-por-voz.md).
//!
//! Claude Code lo arranca por stdio. Sale a preguntar al relevo por los mensajes del teléfono (no
//! abre puertos), los mete a la sesión como `notifications/claude/channel` y devuelve al relevo las
//! respuestas (herramienta `responder`) y las peticiones de permiso. Nunca manda veredictos: el
//! permiso se da en la pantalla.

mod relevo;

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CustomNotification, Implementation, ProtocolVersion, ServerCapabilities, ServerConfig,
    ServerNotification,
};
use rmcp::schemars::{self, JsonSchema};
use rmcp::service::{NotificationContext, Peer};
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use serde::Deserialize;
use serde_json::json;

use relevo::{Mensaje, Permiso, Relevo};

const INSTRUCCIONES: &str = "Los mensajes de este canal llegan como <channel source=\"telefono\" \
    de=\"correo\">: alguien de la papelería te habla desde el teléfono, en modo de voz, y lo que \
    contestes se lo lee en voz alta otro Claude. `de` es el correo de quien habla. Trátalo como si \
    esa persona te lo escribiera en esta terminal: lo que pide confirmación la sigue pidiendo, y se \
    aprueba aquí en la pantalla. Al terminar cada mensaje, llama a la herramienta responder una \
    sola vez con la respuesta: corta (una a tres frases), en español, sin listas, tablas, emojis \
    ni rutas largas, porque se va a oír. Si no pudiste hacerlo, dilo igual por responder.";

const PETICION_DE_PERMISO: &str = "notifications/claude/channel/permission_request";

/// Sin la revisión 2026-07-28: en el runtime v2 de Claude Code, un canal que la negocia no se
/// registra (ver «Versión del protocolo» en el documento).
const VERSIONES: &[ProtocolVersion] = &[
    ProtocolVersion::V_2024_11_05,
    ProtocolVersion::V_2025_03_26,
    ProtocolVersion::V_2025_06_18,
    ProtocolVersion::V_2025_11_25,
];

#[derive(Clone)]
struct Canal {
    relevo: Arc<Relevo>,
}

#[derive(Deserialize, JsonSchema)]
struct ResponderParams {
    /// La respuesta, para leerse en voz alta.
    texto: String,
}

#[derive(Deserialize)]
struct PeticionDePermiso {
    request_id: String,
    tool_name: String,
    description: String,
}

#[tool_router]
impl Canal {
    #[tool(
        description = "Le contesta a quien habló por el teléfono. Se lee en voz alta: corta, en \
                       español, sin listas ni emojis. Llámala una vez al terminar cada mensaje \
                       del canal."
    )]
    async fn responder(
        &self,
        Parameters(ResponderParams { texto }): Parameters<ResponderParams>,
    ) -> Result<String, ErrorData> {
        self.relevo.responder(&texto).await.map_err(|error| {
            tracing::warn!(%error, "no se pudo mandar la respuesta");
            ErrorData::internal_error(format!("no llegó al teléfono: {error}"), None)
        })?;
        Ok("Enviado al teléfono.".into())
    }

    /// Pasa al relevo una petición de permiso de Claude Code, para que el teléfono la avise.
    async fn avisar_permiso(&self, notificacion: CustomNotification) {
        if notificacion.method != PETICION_DE_PERMISO {
            return;
        }
        let Ok(Some(peticion)) = notificacion.params_as::<PeticionDePermiso>() else {
            tracing::warn!(params = ?notificacion.params, "petición de permiso que no se entiende");
            return;
        };
        let permiso = Permiso {
            id: peticion.request_id,
            herramienta: peticion.tool_name,
            descripcion: peticion.description,
        };
        if let Err(error) = self.relevo.pedir_permiso(&permiso).await {
            tracing::warn!(%error, "no se pudo avisar el permiso");
        }
    }
}

#[tool_handler]
impl ServerHandler for Canal {
    fn get_info(&self) -> ServerConfig {
        let mut capacidades = ServerCapabilities::builder().enable_tools().build();
        capacidades.experimental = Some(
            ["claude/channel", "claude/channel/permission"]
                .into_iter()
                .map(|clave| (clave.to_string(), Default::default()))
                .collect(),
        );
        ServerConfig::new(capacidades)
            .with_server_info(Implementation::new(
                "canal-papeleria",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCCIONES)
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(VERSIONES)
    }

    async fn on_custom_notification(
        &self,
        notificacion: CustomNotification,
        _context: NotificationContext<RoleServer>,
    ) {
        self.avisar_permiso(notificacion).await;
    }
}

/// Un mensaje del teléfono como evento del canal: el texto y, en `meta`, quién lo dijo.
fn evento(mensaje: Mensaje) -> ServerNotification {
    ServerNotification::CustomNotification(CustomNotification::new(
        "notifications/claude/channel",
        Some(json!({ "content": mensaje.texto, "meta": { "de": mensaje.de } })),
    ))
}

/// Pregunta al relevo sin parar; así también sabe que la computadora está prendida.
async fn preguntar(peer: Peer<RoleServer>, relevo: Arc<Relevo>) {
    loop {
        match relevo.mensajes().await {
            Ok(mensajes) => {
                for mensaje in mensajes {
                    tracing::info!(de = %mensaje.de, "mensaje del teléfono");
                    if let Err(error) = peer.send_notification(evento(mensaje)).await {
                        tracing::warn!(%error, "no se pudo meter el mensaje a la sesión");
                    }
                }
            }
            Err(error) => {
                let pausa = if relevo::rechazado(&error) { 60 } else { 5 };
                tracing::warn!(%error, pausa, "no se pudo preguntar al relevo");
                tokio::time::sleep(Duration::from_secs(pausa)).await;
            }
        }
    }
}

fn variable(nombre: &str) -> std::io::Result<String> {
    std::env::var(nombre)
        .map_err(|_| std::io::Error::other(format!("falta la variable de entorno {nombre}")))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // stdout es el protocolo con Claude Code: los logs van a stderr.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    // El secreto vive en un archivo propio (600), no en la configuración de Claude Code.
    let archivo = std::env::var("CANAL_SECRETO").unwrap_or_else(|_| {
        format!(
            "{}/.config/canal-papeleria/secreto",
            std::env::var("HOME").unwrap_or_default()
        )
    });
    let secreto = std::fs::read_to_string(&archivo)
        .map_err(|e| std::io::Error::other(format!("no se pudo leer {archivo}: {e}")))?;
    let relevo = Arc::new(Relevo::new(
        &variable("RELEVO_URL")?,
        variable("CANAL_CLIENT_ID")?,
        secreto.trim().to_string(),
    ));

    let servicio = Canal {
        relevo: relevo.clone(),
    }
    .serve(rmcp::transport::stdio())
    .await
    .map_err(std::io::Error::other)?;
    tokio::spawn(preguntar(servicio.peer().clone(), relevo));
    servicio.waiting().await.map_err(std::io::Error::other)?;
    Ok(())
}

#[cfg(test)]
mod tests;
