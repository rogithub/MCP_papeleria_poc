//! El cliente del relevo: las rutas `/canal/*`, con el service token de Cloudflare Access.

use std::time::Duration;

use reqwest::{Method, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, PartialEq)]
pub struct Mensaje {
    /// El correo de quien habló por el teléfono.
    pub de: String,
    pub texto: String,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Permiso {
    pub id: String,
    pub herramienta: String,
    pub descripcion: String,
}

pub struct Relevo {
    base: String,
    client_id: String,
    secreto: String,
    http: reqwest::Client,
}

impl Relevo {
    pub fn new(base: &str, client_id: String, secreto: String) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            client_id,
            secreto,
            // El relevo espera hasta 25 s antes de contestar `GET /canal/mensajes`.
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("cliente HTTP"),
        }
    }

    fn peticion(&self, metodo: Method, ruta: &str) -> RequestBuilder {
        self.http
            .request(metodo, format!("{}{ruta}", self.base))
            .header("CF-Access-Client-Id", &self.client_id)
            .header("CF-Access-Client-Secret", &self.secreto)
    }

    /// Los mensajes del teléfono; el relevo espera hasta 25 s si no hay.
    pub async fn mensajes(&self) -> Result<Vec<Mensaje>, reqwest::Error> {
        self.peticion(Method::GET, "/canal/mensajes")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }

    pub async fn responder(&self, texto: &str) -> Result<(), reqwest::Error> {
        #[derive(Serialize)]
        struct Respuesta<'a> {
            texto: &'a str,
        }
        self.peticion(Method::POST, "/canal/respuesta")
            .json(&Respuesta { texto })
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn pedir_permiso(&self, permiso: &Permiso) -> Result<(), reqwest::Error> {
        self.peticion(Method::POST, "/canal/permiso")
            .json(permiso)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }
}

/// Access o el relevo rechazaron el service token: no se arregla reintentando seguido.
pub fn rechazado(error: &reqwest::Error) -> bool {
    matches!(
        error.status(),
        Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
    )
}
