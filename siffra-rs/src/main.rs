//! Siffra — réplica en Rust del scaffold Next.js (MVP de inteligencia financiera de empresas suecas).
//!
//! Servidor Axum con HTML renderizado en servidor (maud). Todos los datos son de EJEMPLO.

mod format;
mod model;
mod summary;
mod views;

use std::net::SocketAddr;

use axum::{
    extract::{Path, Query},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use maud::Markup;
use serde::Deserialize;

const STYLES: &str = include_str!("../static/styles.css");

fn html(markup: Markup) -> Response {
    Html(markup.into_string()).into_response()
}

fn html_status(status: StatusCode, markup: Markup) -> Response {
    (status, Html(markup.into_string())).into_response()
}

#[derive(Deserialize)]
struct SokParams {
    q: Option<String>,
}

#[derive(Deserialize)]
struct CompanyParams {
    tab: Option<String>,
}

async fn root() -> Redirect {
    Redirect::temporary("/sok")
}

async fn sok(Query(p): Query<SokParams>) -> Response {
    html(views::sok_page(p.q.as_deref().unwrap_or("")))
}

async fn company(Path(org): Path<String>, Query(p): Query<CompanyParams>) -> Response {
    match model::find_example_company(&org) {
        Some(c) => html(views::company_page(c, p.tab.as_deref().unwrap_or("ov"))),
        None => html_status(StatusCode::NOT_FOUND, views::company_not_found_page()),
    }
}

async fn bevakning() -> Response {
    html(views::bevakning_page())
}

async fn likviditet() -> Response {
    html(views::likviditet_page())
}

async fn sie() -> Response {
    html(views::sie_page())
}

async fn fakturor() -> Response {
    html(views::fakturor_page())
}

async fn styles() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], STYLES)
}

async fn fallback() -> Response {
    html_status(StatusCode::NOT_FOUND, views::not_found_page())
}

fn app() -> Router {
    Router::new()
        .route("/", get(root))
        .route("/sok", get(sok))
        .route("/foretag/:org", get(company))
        .route("/bevakning", get(bevakning))
        .route("/likviditet", get(likviditet))
        .route("/sie", get(sie))
        .route("/fakturor", get(fakturor))
        .route("/static/styles.css", get(styles))
        .fallback(fallback)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // 3000 lo usa `npm run dev` del original; por defecto aquí 3001 para poder ejecutar ambos a la vez.
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Siffra (Rust) escuchando en http://localhost:{port}");
    axum::serve(listener, app()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn get_path(path: &str) -> (StatusCode, Option<String>, String) {
        let res = app()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let location = res
            .headers()
            .get(header::LOCATION)
            .map(|v| v.to_str().unwrap().to_string());
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, location, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn root_redirects_to_sok() {
        let (status, location, _) = get_path("/").await;
        assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(location.as_deref(), Some("/sok"));
    }

    #[tokio::test]
    async fn sok_lists_three_example_companies() {
        let (status, _, body) = get_path("/sok").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("<title>Sök företag — Siffra</title>"));
        for name in ["Nordlys Logistik AB", "Fjällbruk Bygg &amp; Design AB", "Kvarn &amp; Krydda Livs AB"] {
            assert!(body.contains(name), "falta {name}");
        }
        assert!(body.contains("Låg risk") && body.contains("Förhöjd risk") && body.contains("Bevaka"));
        assert!(body.contains("EJEMPLO"));
    }

    #[tokio::test]
    async fn sok_filters_by_query() {
        let (_, _, body) = get_path("/sok?q=uppsala").await;
        assert!(body.contains("Kvarn &amp; Krydda Livs AB"));
        assert!(!body.contains("Nordlys Logistik AB"));
        let (_, _, body) = get_path("/sok?q=zzz").await;
        assert!(body.contains("Inga träffar"));
    }

    #[tokio::test]
    async fn company_tabs_render() {
        let (status, _, body) = get_path("/foretag/559108-7721").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("Omsättning, 5 år (tkr)"));
        let (_, _, body) = get_path("/foretag/559108-7721?tab=fin").await;
        assert!(body.contains("Resultat efter finansiella poster"));
        let (_, _, body) = get_path("/foretag/559108-7721?tab=ppl").await;
        assert!(body.contains("Eva Testlund"));
        let (_, _, body) = get_path("/foretag/559108-7721?tab=ai").await;
        assert!(body.contains("Automatisk sammanfattning"));
    }

    #[tokio::test]
    async fn unknown_company_is_404() {
        let (status, _, body) = get_path("/foretag/000000-0000").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.contains("Företaget hittades inte"));
    }

    #[tokio::test]
    async fn static_pages_render() {
        for (path, needle) in [
            ("/bevakning", "Mail varje måndag"),
            ("/likviditet", "Kassan kommer nära <strong>130 tkr</strong> i vecka 12."),
            ("/sie", "Släpp en SIE-fil här"),
            ("/fakturor", "Hamnkraft Test AB"),
        ] {
            let (status, _, body) = get_path(path).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert!(body.contains(needle), "{path} no contiene {needle:?}");
        }
    }

    #[tokio::test]
    async fn serves_stylesheet() {
        let (status, _, body) = get_path("/static/styles.css").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("--accent: #0b6e75"));
    }
}
