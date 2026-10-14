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

#[derive(Serialize)]
struct TestClaims<'a> {
    email: &'a str,
    aud: Vec<&'a str>,
    iss: &'a str,
    exp: u64,
}

fn claims() -> TestClaims<'static> {
    TestClaims {
        email: CORREO,
        aud: vec![AUD],
        iss: EQUIPO,
        exp: get_current_timestamp() + 600,
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
async fn acepta_el_jwt_de_access_y_devuelve_el_correo() {
    let token = sign(&claims(), LLAVE);

    assert_eq!(access().verify(&token).await, Ok(CORREO.to_string()));
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
    let protected = Router::new()
        .route("/mcp", get(|| async { "herramientas" }))
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(access()),
            require_access,
        ));
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .merge(protected)
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
    let request = Request::get("/mcp")
        .header(HEADER, sign(&claims(), LLAVE))
        .body(Body::empty())
        .unwrap();

    assert_eq!(status(request).await, StatusCode::OK);
}
