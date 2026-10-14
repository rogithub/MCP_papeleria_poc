//! Las rutas por donde pregunta el canal de la computadora (ver docs/control-remoto-por-voz.md).
//!
//! JSON simple, no MCP: el canal sale a preguntar (no abre puertos), recoge los mensajes del
//! teléfono y devuelve las respuestas y las peticiones de permiso de la sesión. Solo entra el
//! service token del canal (`acceso::solo_el_canal`).

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::fila::{Fila, Mensaje};

/// Lo que espera `GET /canal/mensajes` si no hay nada: muy por debajo de los 100 s que aguanta
/// Cloudflare, y menos que el minuto que da la computadora por apagada.
const ESPERA: Duration = Duration::from_secs(25);

#[derive(Deserialize)]
struct Respuesta {
    texto: String,
}

#[derive(Deserialize)]
struct Permiso {
    id: String,
    herramienta: String,
    descripcion: String,
}

pub fn rutas(fila: Arc<Fila>) -> Router {
    Router::new()
        .route("/canal/mensajes", get(mensajes))
        .route("/canal/respuesta", post(respuesta))
        .route("/canal/permiso", post(permiso))
        .with_state(fila)
}

async fn mensajes(State(fila): State<Arc<Fila>>) -> Json<Vec<Mensaje>> {
    Json(fila.recoger(ESPERA).await)
}

async fn respuesta(State(fila): State<Arc<Fila>>, Json(r): Json<Respuesta>) -> StatusCode {
    fila.responder(r.texto).await;
    StatusCode::NO_CONTENT
}

async fn permiso(State(fila): State<Arc<Fila>>, Json(p): Json<Permiso>) -> StatusCode {
    fila.pedir_permiso(p.id, p.herramienta, p.descripcion).await;
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests;
