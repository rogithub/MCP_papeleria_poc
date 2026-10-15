use axum::Router;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use serde_json::Value;
use tokio::sync::Mutex;

use super::*;

const CLIENT_ID: &str = "id-del-canal.access";
const SECRETO: &str = "secreto-de-prueba";

/// Lo que el relevo falso recibió: ruta, Client ID, secreto y cuerpo.
type Recibido = Arc<Mutex<Vec<(String, String, String, Value)>>>;

/// Un relevo falso en un puerto libre de 127.0.0.1; `GET /canal/mensajes` devuelve `mensajes`.
async fn relevo_falso(mensajes: Value) -> (Arc<Relevo>, Recibido) {
    let recibido: Recibido = Default::default();
    async fn anotar(
        State((recibido, _)): State<(Recibido, Value)>,
        uri: axum::http::Uri,
        headers: HeaderMap,
        body: String,
    ) -> axum::http::StatusCode {
        let cabecera = |nombre: &str| headers[nombre].to_str().unwrap().to_string();
        let cuerpo = serde_json::from_str(&body).unwrap_or(Value::Null);
        recibido.lock().await.push((
            uri.path().to_string(),
            cabecera("cf-access-client-id"),
            cabecera("cf-access-client-secret"),
            cuerpo,
        ));
        axum::http::StatusCode::NO_CONTENT
    }
    let app = Router::new()
        .route(
            "/canal/mensajes",
            get(|State((_, mensajes)): State<(Recibido, Value)>| async move {
                axum::Json(mensajes)
            }),
        )
        .route("/canal/respuesta", post(anotar))
        .route("/canal/permiso", post(anotar))
        .with_state((recibido.clone(), mensajes));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await });
    let relevo = Relevo::new(&base, CLIENT_ID.into(), SECRETO.into());
    (Arc::new(relevo), recibido)
}

#[test]
fn declara_el_canal_y_el_permiso() {
    let relevo = Arc::new(Relevo::new("http://x", "".into(), "".into()));
    let info = Canal { relevo }.get_info();

    let experimental = info.capabilities.experimental.unwrap();
    assert!(experimental.contains_key("claude/channel"));
    assert!(experimental.contains_key("claude/channel/permission"));
    assert!(info.capabilities.tools.is_some());
    assert!(info.instructions.unwrap().contains("responder"));
}

#[test]
fn no_ofrece_la_revision_2026_07_28() {
    let relevo = Arc::new(Relevo::new("http://x", "".into(), "".into()));
    let versiones = Canal { relevo }.supported_protocol_versions();

    assert!(!versiones.contains(&ProtocolVersion::V_2026_07_28));
    assert!(versiones.contains(&ProtocolVersion::V_2025_11_25));
}

#[tokio::test]
async fn un_mensaje_del_relevo_entra_con_quien_lo_manda() {
    let (relevo, _) = relevo_falso(serde_json::json!([
        {"de": "dayana@example.com", "texto": "imprime el PDF de Juan"}
    ]))
    .await;

    let mut mensajes = relevo.mensajes().await.unwrap();
    let ServerNotification::CustomNotification(n) = evento(mensajes.remove(0)) else {
        panic!("no es una notificación del canal");
    };

    assert_eq!(n.method, "notifications/claude/channel");
    assert_eq!(
        n.params.unwrap(),
        serde_json::json!({
            "content": "imprime el PDF de Juan",
            "meta": {"de": "dayana@example.com"}
        })
    );
}

#[tokio::test]
async fn el_cliente_manda_las_credenciales_de_access() {
    let (relevo, recibido) = relevo_falso(serde_json::json!([])).await;

    relevo.responder("Listo.").await.unwrap();

    let (_, client_id, secreto, _) = recibido.lock().await.remove(0);
    assert_eq!((client_id.as_str(), secreto.as_str()), (CLIENT_ID, SECRETO));
}

#[tokio::test]
async fn responder_manda_el_texto_al_relevo() {
    let (relevo, recibido) = relevo_falso(serde_json::json!([])).await;
    let canal = Canal { relevo };

    let contestacion = canal
        .responder(Parameters(ResponderParams {
            texto: "Listo, dos copias.".into(),
        }))
        .await
        .unwrap();

    assert_eq!(contestacion, "Enviado al teléfono.");
    let (ruta, _, _, cuerpo) = recibido.lock().await.remove(0);
    assert_eq!(ruta, "/canal/respuesta");
    assert_eq!(cuerpo, serde_json::json!({"texto": "Listo, dos copias."}));
}

#[tokio::test]
async fn la_peticion_de_permiso_se_pasa_al_relevo() {
    let (relevo, recibido) = relevo_falso(serde_json::json!([])).await;
    let canal = Canal { relevo };

    canal
        .avisar_permiso(CustomNotification::new(
            PETICION_DE_PERMISO,
            Some(serde_json::json!({
                "request_id": "abcde",
                "tool_name": "Bash",
                "description": "imprimir juan.pdf",
                "input_preview": "{\"command\": \"lp juan.pdf\"}"
            })),
        ))
        .await;

    let (ruta, _, _, cuerpo) = recibido.lock().await.remove(0);
    assert_eq!(ruta, "/canal/permiso");
    assert_eq!(
        cuerpo,
        serde_json::json!({"id": "abcde", "herramienta": "Bash", "descripcion": "imprimir juan.pdf"})
    );
}

#[tokio::test]
async fn otra_notificacion_no_se_pasa_al_relevo() {
    let (relevo, recibido) = relevo_falso(serde_json::json!([])).await;
    let canal = Canal { relevo };

    canal
        .avisar_permiso(CustomNotification::new("notifications/otra", None))
        .await;

    assert!(recibido.lock().await.is_empty());
}

#[test]
fn solo_pregunta_si_lo_abrio_claude_telefono() {
    assert!(activo(Some("1")));
    assert!(!activo(None));
    assert!(!activo(Some("")));
}
