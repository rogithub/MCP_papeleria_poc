//! La fila entre el teléfono y la sesión de Claude Code (ver docs/control-remoto-por-voz.md).
//!
//! En memoria y una sola conversación: si el pod se reinicia, lo pendiente se pierde, y si dos
//! personas hablan a la vez, sus respuestas se mezclan. Para el experimento basta.
//!
//! El canal de la computadora pregunta seguido por mensajes nuevos; si no ha preguntado en el
//! último minuto, la computadora se da por apagada (fuera de horario no tiene corriente).

use std::collections::VecDeque;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{Mutex, Notify};
use tokio::time::{Instant, timeout_at};

const PRESENCIA: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Mensaje {
    /// El correo de quien habló, como lo dio Access.
    pub de: String,
    pub texto: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Salida {
    Respuesta(String),
    Permiso {
        id: String,
        herramienta: String,
        descripcion: String,
    },
}

#[derive(Default)]
pub struct Fila {
    estado: Mutex<Estado>,
    hay_entrada: Notify,
    hay_salida: Notify,
}

#[derive(Default)]
struct Estado {
    entrada: VecDeque<Mensaje>,
    salida: Vec<Salida>,
    ultimo_contacto: Option<Instant>,
}

impl Estado {
    fn prendida(&self) -> bool {
        self.ultimo_contacto
            .is_some_and(|at| at.elapsed() < PRESENCIA)
    }
}

impl Fila {
    pub async fn prendida(&self) -> bool {
        self.estado.lock().await.prendida()
    }

    /// Mete un mensaje para la sesión. Con la computadora apagada no se guarda: nadie lo
    /// recogería, y llegaría horas después, fuera de contexto.
    ///
    /// Lo que la sesión devolvió y el teléfono nunca leyó se descarta: si no, el teléfono lo
    /// tomaría como la respuesta a este mensaje y las respuestas quedarían corridas una vuelta.
    pub async fn mandar(&self, mensaje: Mensaje) -> bool {
        let mut estado = self.estado.lock().await;
        if !estado.prendida() {
            return false;
        }
        for salida in estado.salida.drain(..) {
            tracing::warn!(?salida, "descartado: nadie lo leyó antes del mensaje nuevo");
        }
        estado.entrada.push_back(mensaje);
        self.hay_entrada.notify_one();
        true
    }

    /// Lo que pide el canal: los mensajes pendientes, esperando hasta `espera` si no hay.
    pub async fn recoger(&self, espera: Duration) -> Vec<Mensaje> {
        self.contacto().await;
        let mensajes = esperar(&self.estado, &self.hay_entrada, espera, |e| {
            e.entrada.drain(..).collect()
        })
        .await;
        self.contacto().await;
        mensajes
    }

    pub async fn responder(&self, texto: String) {
        self.salir(Salida::Respuesta(texto)).await;
    }

    pub async fn pedir_permiso(&self, id: String, herramienta: String, descripcion: String) {
        self.salir(Salida::Permiso {
            id,
            herramienta,
            descripcion,
        })
        .await;
    }

    /// Lo que pide el teléfono: lo que la sesión haya devuelto desde la última vez, esperando
    /// hasta `espera` si todavía no hay nada.
    pub async fn esperar_salida(&self, espera: Duration) -> Vec<Salida> {
        esperar(&self.estado, &self.hay_salida, espera, |e| {
            std::mem::take(&mut e.salida)
        })
        .await
    }

    async fn salir(&self, salida: Salida) {
        let mut estado = self.estado.lock().await;
        estado.ultimo_contacto = Some(Instant::now());
        estado.salida.push(salida);
        self.hay_salida.notify_one();
    }

    async fn contacto(&self) {
        self.estado.lock().await.ultimo_contacto = Some(Instant::now());
    }
}

/// Saca lo que haya con `sacar`; si sale vacío, espera un aviso y vuelve a ver, hasta el plazo.
async fn esperar<T>(
    estado: &Mutex<Estado>,
    aviso: &Notify,
    espera: Duration,
    sacar: impl Fn(&mut Estado) -> Vec<T>,
) -> Vec<T> {
    let plazo = Instant::now() + espera;
    loop {
        let sacado = sacar(&mut *estado.lock().await);
        if !sacado.is_empty() {
            return sacado;
        }
        if timeout_at(plazo, aviso.notified()).await.is_err() {
            return Vec::new();
        }
    }
}

/// Lo que se le contesta al Claude del teléfono, para decirse en voz alta.
pub fn para_decir(salida: &[Salida]) -> String {
    if salida.is_empty() {
        return "Sigue trabajando. Pregunta otra vez con esperar_respuesta.".into();
    }
    salida
        .iter()
        .map(|s| match s {
            Salida::Respuesta(texto) => texto.clone(),
            Salida::Permiso {
                herramienta,
                descripcion,
                ..
            } => format!(
                "Pide permiso para usar {herramienta}: {descripcion}. Se aprueba o se rechaza \
                 en la pantalla de la computadora."
            ),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests;
