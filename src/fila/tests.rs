use std::sync::Arc;

use tokio::time::advance;

use super::*;

const ESPERA: Duration = Duration::from_secs(20);

fn mensaje(texto: &str) -> Mensaje {
    Mensaje {
        de: "dayana@example.com".into(),
        texto: texto.into(),
    }
}

/// Una fila con la computadora prendida: el canal acaba de preguntar.
async fn prendida() -> Fila {
    let fila = Fila::default();
    fila.recoger(Duration::ZERO).await;
    fila
}

#[tokio::test(start_paused = true)]
async fn sin_contacto_la_computadora_esta_apagada() {
    assert!(!Fila::default().prendida().await);
}

#[tokio::test(start_paused = true)]
async fn con_contacto_hace_menos_de_un_minuto_esta_prendida() {
    let fila = prendida().await;
    advance(Duration::from_secs(59)).await;

    assert!(fila.prendida().await);
}

#[tokio::test(start_paused = true)]
async fn sin_contacto_en_un_minuto_se_da_por_apagada() {
    let fila = prendida().await;
    advance(Duration::from_secs(61)).await;

    assert!(!fila.prendida().await);
}

#[tokio::test(start_paused = true)]
async fn con_la_computadora_apagada_el_mensaje_no_se_guarda() {
    let fila = Fila::default();

    assert!(!fila.mandar(mensaje("imprime el PDF de Juan")).await);
    assert!(fila.recoger(Duration::ZERO).await.is_empty());
}

#[tokio::test(start_paused = true)]
async fn el_mensaje_lleva_quien_lo_manda_y_se_recoge_una_sola_vez() {
    let fila = prendida().await;

    assert!(fila.mandar(mensaje("imprime el PDF de Juan")).await);

    assert_eq!(
        fila.recoger(Duration::ZERO).await,
        vec![mensaje("imprime el PDF de Juan")]
    );
    assert!(fila.recoger(Duration::ZERO).await.is_empty());
}

#[tokio::test(start_paused = true)]
async fn el_canal_espera_hasta_que_llega_un_mensaje() {
    let fila = Arc::new(prendida().await);
    let canal = tokio::spawn({
        let fila = fila.clone();
        async move { fila.recoger(Duration::from_secs(25)).await }
    });
    advance(Duration::from_secs(10)).await;

    fila.mandar(mensaje("¿ya quedó?")).await;

    assert_eq!(canal.await.unwrap(), vec![mensaje("¿ya quedó?")]);
}

#[tokio::test(start_paused = true)]
async fn el_canal_recibe_vacio_si_no_llega_nada() {
    let fila = prendida().await;

    assert!(fila.recoger(Duration::from_secs(25)).await.is_empty());
}

#[tokio::test(start_paused = true)]
async fn mientras_el_canal_espera_la_computadora_sigue_prendida() {
    let fila = prendida().await;
    fila.recoger(Duration::from_secs(25)).await;
    advance(Duration::from_secs(50)).await;

    assert!(fila.prendida().await);
}

#[tokio::test(start_paused = true)]
async fn la_respuesta_despierta_al_telefono_que_espera() {
    let fila = Arc::new(prendida().await);
    let telefono = tokio::spawn({
        let fila = fila.clone();
        async move { fila.esperar_salida(ESPERA).await }
    });
    advance(Duration::from_secs(5)).await;

    fila.responder("Listo, dos copias a color.".into()).await;

    assert_eq!(
        telefono.await.unwrap(),
        vec![Salida::Respuesta("Listo, dos copias a color.".into())]
    );
}

#[tokio::test(start_paused = true)]
async fn sin_respuesta_en_veinte_segundos_dice_que_sigue_trabajando() {
    let fila = prendida().await;

    let salida = fila.esperar_salida(ESPERA).await;

    assert_eq!(
        para_decir(&salida),
        "Sigue trabajando. Pregunta otra vez con esperar_respuesta."
    );
}

#[tokio::test(start_paused = true)]
async fn una_peticion_de_permiso_se_avisa_para_aprobar_en_pantalla() {
    let fila = prendida().await;
    fila.pedir_permiso("abcde".into(), "Bash".into(), "imprimir juan.pdf".into())
        .await;

    let salida = fila.esperar_salida(ESPERA).await;

    assert_eq!(
        para_decir(&salida),
        "Pide permiso para usar Bash: imprimir juan.pdf. Se aprueba o se rechaza en la \
         pantalla de la computadora. Sigue trabajando: en cuanto se apruebe, pregunta otra vez \
         con esperar_respuesta para saber cómo terminó."
    );
}

#[tokio::test(start_paused = true)]
async fn lo_acumulado_se_entrega_junto_y_una_sola_vez() {
    let fila = prendida().await;
    fila.responder("Ya lo abrí.".into()).await;
    fila.responder("Ya lo mandé a imprimir.".into()).await;

    let salida = fila.esperar_salida(ESPERA).await;

    assert_eq!(para_decir(&salida), "Ya lo abrí.\nYa lo mandé a imprimir.");
    assert!(fila.esperar_salida(Duration::ZERO).await.is_empty());
}

#[tokio::test(start_paused = true)]
async fn un_mensaje_nuevo_descarta_lo_que_nadie_leyo() {
    let fila = prendida().await;
    fila.responder("Son las 7:03.".into()).await;

    fila.mandar(mensaje("crea el archivo")).await;

    assert!(fila.esperar_salida(Duration::ZERO).await.is_empty());
}

#[tokio::test(start_paused = true)]
async fn lo_que_llega_despues_del_mensaje_si_se_entrega() {
    let fila = prendida().await;
    fila.responder("Son las 7:03.".into()).await;
    fila.mandar(mensaje("crea el archivo")).await;

    fila.responder("Listo, creé el archivo.".into()).await;

    assert_eq!(
        fila.esperar_salida(Duration::ZERO).await,
        vec![Salida::Respuesta("Listo, creé el archivo.".into())]
    );
}
