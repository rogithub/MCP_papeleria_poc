//! Solo pasa a `/mcp` lo que viene de Cloudflare Access (ver docs/control-remoto-por-voz.md).
//!
//! Access hace el login y el OAuth; a cada petición que deja pasar le agrega la cabecera
//! `Cf-Access-Jwt-Assertion`, un JWT firmado con las llaves del equipo. Aquí se revisa esa firma,
//! que sea de esta aplicación (`aud`) y de este equipo (`iss`), y que no haya vencido: el NodePort
//! se alcanza desde la LAN sin pasar por Cloudflare, así que no basta con que Access esté enfrente.
//!
//! Entran dos clases de identidad: las personas de la política (el JWT trae su `email`), que usan
//! `/mcp`, y el canal de la computadora con su service token (trae `common_name`, el Client ID),
//! que usa `/canal`. Ninguna puede entrar a lo de la otra.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};

const HEADER: &str = "cf-access-jwt-assertion";

/// Cloudflare rota sus llaves cada pocas semanas: si llega una que no se conoce, se vuelven a
/// bajar, pero no más de una vez por minuto (alguien en la LAN podría mandar `kid` inventados).
const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

pub struct Access {
    certs_url: String,
    validation: Validation,
    keys: RwLock<HashMap<String, DecodingKey>>,
    last_refresh: Mutex<Option<Instant>>,
    http: reqwest::Client,
}

#[derive(Debug, PartialEq)]
pub enum Rejection {
    MissingHeader,
    UnknownKey,
    Invalid(String),
}

/// Quién entró, en las extensiones de la petición: así el mensaje que llega a la sesión dice
/// quién habló.
#[derive(Clone, Debug, PartialEq)]
pub enum Identidad {
    /// Una persona de la política, con su correo.
    Persona(String),
    /// Un service token, con su Client ID.
    Canal(String),
}

#[derive(Deserialize)]
struct Claims {
    email: Option<String>,
    common_name: Option<String>,
}

impl Access {
    /// `team_domain` es el emisor tal cual, p. ej. `https://negrita.cloudflareaccess.com`; `aud` es
    /// la «AUD tag» de la aplicación. Baja las llaves al arrancar: sin ellas no tiene caso servir.
    pub async fn new(team_domain: &str, aud: &str) -> Result<Self, reqwest::Error> {
        let team_domain = team_domain.trim_end_matches('/');
        let access = Self::with_keys(team_domain, aud, HashMap::new());
        access.refresh().await?;
        Ok(access)
    }

    fn with_keys(team_domain: &str, aud: &str, keys: HashMap<String, DecodingKey>) -> Self {
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[aud]);
        validation.set_issuer(&[team_domain]);
        validation.set_required_spec_claims(&["exp", "aud", "iss"]);
        Self {
            certs_url: format!("{team_domain}/cdn-cgi/access/certs"),
            validation,
            keys: RwLock::new(keys),
            last_refresh: Mutex::new(None),
            http: reqwest::Client::new(),
        }
    }

    pub async fn verify(&self, token: &str) -> Result<Identidad, Rejection> {
        match self.verify_with_known_keys(token).await {
            Err(Rejection::UnknownKey) if self.refresh_if_due().await => {
                self.verify_with_known_keys(token).await
            }
            result => result,
        }
    }

    async fn verify_with_known_keys(&self, token: &str) -> Result<Identidad, Rejection> {
        verify_token(&*self.keys.read().await, &self.validation, token)
    }

    async fn refresh_if_due(&self) -> bool {
        let mut last = self.last_refresh.lock().await;
        if last.is_some_and(|at| at.elapsed() < REFRESH_INTERVAL) {
            return false;
        }
        *last = Some(Instant::now());
        drop(last);
        match self.refresh().await {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, "no se pudieron bajar las llaves de Access");
                false
            }
        }
    }

    async fn refresh(&self) -> Result<(), reqwest::Error> {
        let jwks: JwkSet = self
            .http
            .get(&self.certs_url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let keys = keys_from_jwks(&jwks);
        tracing::info!(llaves = keys.len(), "llaves de Access cargadas");
        *self.keys.write().await = keys;
        Ok(())
    }
}

fn keys_from_jwks(jwks: &JwkSet) -> HashMap<String, DecodingKey> {
    jwks.keys
        .iter()
        .filter_map(|jwk| {
            let kid = jwk.common.key_id.clone()?;
            let key = DecodingKey::from_jwk(jwk).ok()?;
            Some((kid, key))
        })
        .collect()
}

fn verify_token(
    keys: &HashMap<String, DecodingKey>,
    validation: &Validation,
    token: &str,
) -> Result<Identidad, Rejection> {
    let header = decode_header(token).map_err(|e| Rejection::Invalid(e.to_string()))?;
    let key = header
        .kid
        .and_then(|kid| keys.get(&kid))
        .ok_or(Rejection::UnknownKey)?;
    let claims = decode::<Claims>(token, key, validation)
        .map_err(|e| Rejection::Invalid(e.to_string()))?
        .claims;
    match (claims.email, claims.common_name) {
        (Some(correo), _) => Ok(Identidad::Persona(correo)),
        (None, Some(client_id)) => Ok(Identidad::Canal(client_id)),
        (None, None) => Err(Rejection::Invalid("sin email ni common_name".into())),
    }
}

/// Primera capa: sin un JWT válido de Access, 401 y no llega a nada.
pub async fn require_access(
    State(access): State<Arc<Access>>,
    mut request: Request,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get(HEADER)
        .and_then(|value| value.to_str().ok());
    let result = match token {
        Some(token) => access.verify(token).await,
        None => Err(Rejection::MissingHeader),
    };
    match result {
        Ok(identidad) => {
            tracing::info!(?identidad, "petición por Access");
            request.extensions_mut().insert(identidad);
            next.run(request).await
        }
        Err(rejection) => {
            tracing::warn!(?rejection, "petición rechazada: no viene de Access");
            StatusCode::UNAUTHORIZED.into_response()
        }
    }
}

/// Capa para `/mcp`: solo personas; el service token del canal no usa las herramientas.
pub async fn solo_personas(request: Request, next: Next) -> Response {
    match request.extensions().get::<Identidad>() {
        Some(Identidad::Persona(_)) => next.run(request).await,
        otra => {
            tracing::warn!(identidad = ?otra, "rechazada: /mcp es solo para personas");
            StatusCode::FORBIDDEN.into_response()
        }
    }
}

/// Capa para `/canal`: solo el service token del canal (`client_id`), no otro ni una persona.
pub async fn solo_el_canal(
    State(client_id): State<Arc<str>>,
    request: Request,
    next: Next,
) -> Response {
    match request.extensions().get::<Identidad>() {
        Some(Identidad::Canal(id)) if **id == *client_id => next.run(request).await,
        otra => {
            tracing::warn!(identidad = ?otra, "rechazada: /canal es solo para el canal");
            StatusCode::FORBIDDEN.into_response()
        }
    }
}

#[cfg(test)]
mod tests;
