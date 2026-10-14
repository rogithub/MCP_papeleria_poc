use axum::Router;
use axum::body::Body;
use axum::http::Request;
use axum::routing::get;
use jsonwebtoken::{EncodingKey, Header, encode, get_current_timestamp};
use serde::Serialize;
use tower::ServiceExt;

use super::*;

// Llaves solo de prueba, generadas para estas pruebas; no firman nada real.
const LLAVE: &[u8] = include_bytes!("llave_de_prueba.pem");
const OTRA_LLAVE: &[u8] = include_bytes!("otra_llave_de_prueba.pem");
const JWKS: &str = include_str!("jwks_de_prueba.json");
const KID: &str = "llave-de-prueba";

const EQUIPO: &str = "https://negrita.cloudflareaccess.com";
const AUD: &str = "aud-de-la-aplicacion";
const CORREO: &str = "dueno@example.com";
const CANAL: &str = "id-del-canal.access";

#[derive(Serialize)]
struct TestClaims<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    common_name: Option<&'a str>,
    aud: Vec<&'a str>,
    iss: &'a str,
    exp: u64,
}

fn claims() -> TestClaims<'static> {
    TestClaims {
        email: Some(CORREO),
        common_name: None,
        aud: vec![AUD],
        iss: EQUIPO,
        exp: get_current_timestamp() + 600,
    }
}

/// Lo que manda Access cuando entra un service token: `common_name` y sin `email`.
fn claims_del_service_token(client_id: &'static str) -> TestClaims<'static> {
    TestClaims {
        email: None,
        common_name: Some(client_id),
        ..claims()
    }
}

fn sign(claims: &TestClaims, pem: &[u8]) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KID.into());
    encode(&header, claims, &EncodingKey::from_rsa_pem(pem).unwrap()).unwrap()
}

fn access() -> Access {
    let jwks: JwkSet = serde_json::from_str(JWKS).unwrap();
    Access::with_keys(EQUIPO, AUD, keys_from_jwks(&jwks))
}

#[tokio::test]
async fn el_jwt_de_una_persona_trae_su_correo() {
    let token = sign(&claims(), LLAVE);

    assert_eq!(
        access().verify(&token).await,
        Ok(Identidad::Persona(CORREO.into()))
    );
}

#[tokio::test]
async fn el_jwt_de_un_service_token_se_reconoce_como_canal() {
    let token = sign(&claims_del_service_token(CANAL), LLAVE);

    assert_eq!(
        access().verify(&token).await,
        Ok(Identidad::Canal(CANAL.into()))
    );
}

#[tokio::test]
async fn rechaza_un_jwt_firmado_con_otra_llave() {
    let token = sign(&claims(), OTRA_LLAVE);

    assert!(matches!(
        access().verify(&token).await,
        Err(Rejection::Invalid(_))
    ));
}

#[tokio::test]
async fn rechaza_un_jwt_de_otra_aplicacion() {
    let token = sign(
        &TestClaims {
            aud: vec!["otra-aplicacion"],
            ..claims()
        },
        LLAVE,
    );

    assert!(matches!(
        access().verify(&token).await,
        Err(Rejection::Invalid(_))
    ));
}

#[tokio::test]
async fn rechaza_un_jwt_de_otro_equipo() {
    let token = sign(
        &TestClaims {
            iss: "https://otro.cloudflareaccess.com",
            ..claims()
        },
        LLAVE,
    );

    assert!(matches!(
        access().verify(&token).await,
        Err(Rejection::Invalid(_))
    ));
}

#[tokio::test]
async fn rechaza_un_jwt_vencido() {
    let token = sign(
        &TestClaims {
            exp: get_current_timestamp() - 600,
            ..claims()
        },
        LLAVE,
    );

    assert!(matches!(
        access().verify(&token).await,
        Err(Rejection::Invalid(_))
    ));
}

#[tokio::test]
async fn rechaza_un_jwt_con_una_llave_que_no_conoce() {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("llave-inventada".into());
    let token = encode(
        &header,
        &claims(),
        &EncodingKey::from_rsa_pem(LLAVE).unwrap(),
    )
    .unwrap();
    let access = access();
    // Como si ya se hubieran bajado hace un momento: no sale a internet en la prueba.
    *access.last_refresh.lock().await = Some(Instant::now());

    assert_eq!(access.verify(&token).await, Err(Rejection::UnknownKey));
}

fn app() -> Router {
    let mcp = Router::new()
        .route("/mcp", get(|| async { "herramientas" }))
        .route(
            "/quien",
            get(
                |axum::Extension(identidad): axum::Extension<Identidad>| async move {
                    format!("{identidad:?}")
                },
            ),
        )
        .layer(axum::middleware::from_fn(solo_personas));
    let canal = Router::new()
        .route("/canal/mensajes", get(|| async { "[]" }))
        .layer(axum::middleware::from_fn_with_state(
            Arc::<str>::from(CANAL),
            solo_el_canal,
        ));
    let protected = mcp.merge(canal).layer(axum::middleware::from_fn_with_state(
        Arc::new(access()),
        require_access,
    ));
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .merge(protected)
}

fn get_con(ruta: &str, claims: &TestClaims) -> Request<Body> {
    Request::get(ruta)
        .header(HEADER, sign(claims, LLAVE))
        .body(Body::empty())
        .unwrap()
}

async fn status(request: Request<Body>) -> StatusCode {
    app().oneshot(request).await.unwrap().status()
}

#[tokio::test]
async fn healthz_responde_sin_jwt() {
    let request = Request::get("/healthz").body(Body::empty()).unwrap();

    assert_eq!(status(request).await, StatusCode::OK);
}

#[tokio::test]
async fn mcp_sin_la_cabecera_de_access_responde_401() {
    let request = Request::get("/mcp").body(Body::empty()).unwrap();

    assert_eq!(status(request).await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mcp_con_el_jwt_de_access_pasa() {
    assert_eq!(status(get_con("/mcp", &claims())).await, StatusCode::OK);
}

#[tokio::test]
async fn el_correo_de_quien_entro_llega_a_las_herramientas() {
    let response = app().oneshot(get_con("/quien", &claims())).await.unwrap();
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();

    assert_eq!(body, format!("Persona(\"{CORREO}\")"));
}

#[tokio::test]
async fn el_canal_no_puede_usar_las_herramientas_del_telefono() {
    let canal = claims_del_service_token(CANAL);

    assert_eq!(status(get_con("/mcp", &canal)).await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn una_persona_no_puede_entrar_a_las_rutas_del_canal() {
    assert_eq!(
        status(get_con("/canal/mensajes", &claims())).await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn un_service_token_ajeno_no_entra_al_canal() {
    let ajeno = claims_del_service_token("otro-token.access");

    assert_eq!(
        status(get_con("/canal/mensajes", &ajeno)).await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn el_canal_entra_a_sus_rutas() {
    let canal = claims_del_service_token(CANAL);

    assert_eq!(
        status(get_con("/canal/mensajes", &canal)).await,
        StatusCode::OK
    );
}
