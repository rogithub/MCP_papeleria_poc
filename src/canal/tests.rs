use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

use super::*;
use crate::fila::Salida;

/// Una fila con la computadora prendida: el canal acaba de preguntar.
async fn prendida() -> Arc<Fila> {
    let fila = Arc::new(Fila::default());
    fila.recoger(Duration::ZERO).await;
    fila
}

fn post_json(ruta: &str, json: &str) -> Request<Body> {
    Request::post(ruta)
        .header("content-type", "application/json")
        .body(Body::from(json.to_string()))
        .unwrap()
}

#[tokio::test(start_paused = true)]
async fn el_canal_recoge_mensajes_por_http_con_quien_los_manda() {
    let fila = prendida().await;
    fila.mandar(Mensaje {
        de: "dayana@example.com".into(),
        texto: "imprime el PDF de Juan".into(),
    })
    .await;

    let request = Request::get("/canal/mensajes").body(Body::empty()).unwrap();
    let response = rutas(fila).oneshot(request).await.unwrap();
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();

    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!([{"de": "dayana@example.com", "texto": "imprime el PDF de Juan"}])
    );
}

#[tokio::test(start_paused = true)]
async fn sin_mensajes_el_canal_recibe_una_lista_vacia() {
    let request = Request::get("/canal/mensajes").body(Body::empty()).unwrap();
    let response = rutas(prendida().await).oneshot(request).await.unwrap();
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();

    assert_eq!(&body[..], b"[]");
}

#[tokio::test(start_paused = true)]
async fn la_respuesta_del_canal_llega_a_la_fila() {
    let fila = prendida().await;

    let response = rutas(fila.clone())
        .oneshot(post_json("/canal/respuesta", r#"{"texto": "Listo."}"#))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        fila.esperar_salida(Duration::ZERO).await,
        vec![Salida::Respuesta("Listo.".into())]
    );
}

#[tokio::test(start_paused = true)]
async fn el_permiso_del_canal_llega_a_la_fila() {
    let fila = prendida().await;

    let response = rutas(fila.clone())
        .oneshot(post_json(
            "/canal/permiso",
            r#"{"id": "abcde", "herramienta": "Bash", "descripcion": "imprimir juan.pdf"}"#,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        fila.esperar_salida(Duration::ZERO).await,
        vec![Salida::Permiso {
            id: "abcde".into(),
            herramienta: "Bash".into(),
            descripcion: "imprimir juan.pdf".into(),
        }]
    );
}
