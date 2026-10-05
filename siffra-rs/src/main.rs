//! Siffra — réplica en Rust del scaffold Next.js (MVP de inteligencia financiera de empresas suecas).
//!
//! Servidor Axum con HTML renderizado en servidor (maud). Todos los datos son de EJEMPLO.

mod bolagsverket;
mod format;
mod model;
mod scb;
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
    sort: Option<String>,
    dir: Option<String>,
}

#[derive(Deserialize)]
struct CompanyParams {
    tab: Option<String>,
}

async fn root() -> Redirect {
    Redirect::temporary("/sok")
}

/// Lee `.env.local` (en la carpeta actual o en la raíz del repositorio) y define las variables que
/// aún no existan en el entorno. Es el mismo archivo que usa el proyecto Next.js; git lo ignora.
pub(crate) fn load_dotenv() {
    for path in [".env.local", "../.env.local"] {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        for line in text.trim_start_matches('\u{feff}').lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim(), value.trim().trim_matches(|c| c == '"' || c == '\''));
            if !key.is_empty() && std::env::var_os(key).is_none() {
                std::env::set_var(key, value);
            }
        }
        return;
    }
}

async fn sok(Query(p): Query<SokParams>) -> Response {
    let q = p.q.as_deref().unwrap_or("");
    // Un organisationsnummer que no es de EJEMPLO se busca en Bolagsverket (si hay credenciales).
    // Este API solo consulta por número, no por nombre.
    let live = if model::search_example_companies(q).is_empty() && bolagsverket::configured() {
        match bolagsverket::normalize_org_number(q) {
            Some(n) => match bolagsverket::get_organisation_by_number(&n).await {
                Ok(o) => Some(o),
                Err(bolagsverket::BvError::NotFound(_) | bolagsverket::BvError::Invalid(_)) => None,
                Err(e) => {
                    eprintln!("{e}");
                    None
                }
            },
            None => None,
        }
    } else {
        None
    };
    html(views::sok_page(q, p.sort.as_deref(), p.dir.as_deref(), live.as_ref()))
}

/// Empresa que no es de EJEMPLO: ficha real de Bolagsverket si hay credenciales; si no, 404 como antes.
async fn live_company(org: &str) -> Response {
    if !bolagsverket::configured() {
        return html_status(StatusCode::NOT_FOUND, views::company_not_found_page());
    }
    match bolagsverket::get_organisation_by_number(org).await {
        Ok(o) => html(views::live_profile_page(&o)),
        Err(bolagsverket::BvError::NotFound(_) | bolagsverket::BvError::Invalid(_)) => {
            html_status(StatusCode::NOT_FOUND, views::company_not_found_page())
        }
        Err(e) => {
            eprintln!("{e}");
            // Límite de 60 peticiones/minuto superado → 429; cualquier otro fallo del API → 502.
            let status = match e {
                bolagsverket::BvError::Upstream { status: Some(429), .. } => StatusCode::TOO_MANY_REQUESTS,
                _ => StatusCode::BAD_GATEWAY,
            };
            html_status(status, views::live_error_page())
        }
    }
}

async fn company(Path(org): Path<String>, Query(p): Query<CompanyParams>) -> Response {
    match model::find_example_company(&org) {
        Some(c) => {
            // Medianas reales del sector (SCB). Si ya están en caché la ficha sale completa al instante;
            // si no, sale con un esqueleto y el navegador pide el fragmento (`/foretag/:org/benchmarks`),
            // para no bloquear la página 2-4 s esperando a SCB.
            let bench = if !scb::enabled() {
                views::Bench::Example
            } else {
                match scb::peek(c.sni, c.employee_range) {
                    Some(m) => views::Bench::Ready(m),
                    None => views::Bench::Pending,
                }
            };
            html(views::company_page(c, p.tab.as_deref().unwrap_or("ov"), &bench))
        }
        None => live_company(&org).await,
    }
}

/// Fragmento HTML con la comparación con el sector (lo pide la ficha cuando SCB aún no estaba en caché).
async fn company_benchmarks(Path(org): Path<String>) -> Response {
    let Some(c) = model::find_example_company(&org) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (medians, notice) = if scb::enabled() {
        match scb::get_sector_medians(c.sni, c.employee_range).await {
            Ok(m) => (m, None),
            Err(e) => {
                eprintln!("{e}");
                (None, Some("Kunde inte hämta SCB-data just nu. Visar exempelvärden."))
            }
        }
    } else {
        (None, None)
    };
    html(views::benchmark_fragment(c, medians.as_ref(), notice))
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
        .route("/foretag/:org/benchmarks", get(company_benchmarks))
        .route("/bevakning", get(bevakning))
        .route("/likviditet", get(likviditet))
        .route("/sie", get(sie))
        .route("/fakturor", get(fakturor))
        .route("/static/styles.css", get(styles))
        .fallback(fallback)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    load_dotenv();
    match bolagsverket::environment_host() {
        Some(host) if bolagsverket::configured() => println!("Bolagsverket: conectado a {host}"),
        _ => println!("Bolagsverket: sin credenciales (solo datos de EJEMPLO)"),
    }
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

    /// Los tests que dependen de las variables BOLAGSVERKET_* (globales del proceso) no pueden correr a la vez.
    static BOLAGSVERKET_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock_bolagsverket_env() -> std::sync::MutexGuard<'static, ()> {
        BOLAGSVERKET_ENV.lock().unwrap_or_else(|e| e.into_inner())
    }

    async fn get_path(path: &str) -> (StatusCode, Option<String>, String) {
        // Las pruebas de rutas no deben depender de la red: SCB desactivado → medianas de EJEMPLO.
        std::env::set_var("SCB_STATS_DISABLED", "1");
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
    async fn overview_falls_back_to_example_medians_without_scb() {
        let (_, _, body) = get_path("/foretag/559012-3456").await;
        assert!(body.contains("Strecket visar medianen för SNI 52.290."));
        assert!(!body.contains("Källa: SCB"));
    }

    #[tokio::test]
    async fn unknown_company_is_404() {
        // Sin claves de Bolagsverket no se consulta a nadie: un número desconocido es 404.
        let _env = lock_bolagsverket_env();
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
    async fn sok_sorts_by_revenue_and_marks_aria_sort() {
        // Solo dentro de la tabla (el placeholder del buscador también menciona "Nordlys").
        let pos = |body: &str, name: &str| {
            let table = &body[body.find("<tbody").expect("tbody")..];
            table.find(name).unwrap_or_else(|| panic!("falta {name}"))
        };
        let (_, _, asc) = get_path("/sok?sort=revenue&dir=asc").await;
        assert!(pos(&asc, "Kvarn") < pos(&asc, "Nordlys") && pos(&asc, "Nordlys") < pos(&asc, "Fjällbruk"));
        assert!(asc.contains(r#"aria-sort="ascending""#));
        let (_, _, desc) = get_path("/sok?sort=revenue&dir=desc").await;
        assert!(pos(&desc, "Fjällbruk") < pos(&desc, "Nordlys") && pos(&desc, "Nordlys") < pos(&desc, "Kvarn"));
        assert!(desc.contains(r#"aria-sort="descending""#));
        // Una columna desconocida se ignora (orden original).
        let (_, _, other) = get_path("/sok?sort=nope").await;
        assert!(!other.contains(r#"aria-sort=""#));
    }

    #[tokio::test]
    async fn pages_have_accessible_structure() {
        let (_, _, body) = get_path("/sok").await;
        assert!(body.contains(r##"href="#main""##), "enlace para saltar al contenido");
        assert_eq!(body.matches("<h1").count(), 1, "un único h1");
        assert!(body.contains(r#"aria-label="Huvudmeny""#));
        assert!(body.contains(r#"aria-current="page""#));
        // La navegación solo marca la página actual; las demás no llevan aria-current="false".
        assert!(!body.contains(r#"aria-current="false""#));
        // La ficha cuenta como parte de "Sök företag" y las pestañas son navegación, no role=tab.
        let (_, _, co) = get_path("/foretag/559012-3456?tab=fin").await;
        assert!(co.contains(r#"aria-label="Företagsvyer""#));
        assert!(!co.contains(r#"role="tab""#));
        assert!(co.contains("<caption"));
    }

    #[tokio::test]
    async fn benchmark_fragment_route() {
        let (status, _, body) = get_path("/foretag/559108-7721/benchmarks").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("Vinstmarginal") && body.contains("Kassalikviditet"));
        assert!(!body.contains("<html"), "es un fragmento, no un documento");
        let (status, _, _) = get_path("/foretag/000000-0000/benchmarks").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    /// Flujo completo contra un Bolagsverket SIMULADO en local (OAuth + `/organisationer` con la forma del
    /// ejemplo oficial). No toca la red real. Un solo test porque modifica variables de entorno.
    #[tokio::test]
    async fn live_bolagsverket_flow_with_fake_api() {
        use axum::{http::HeaderMap, routing::post, Form, Json};
        use serde_json::{json, Value};
        use std::collections::HashMap;

        let _env = lock_bolagsverket_env();

        async fn fake_token(Form(f): Form<HashMap<String, String>>) -> Response {
            let ok = f.get("grant_type").map(String::as_str) == Some("client_credentials")
                && f.get("client_id").map(String::as_str) == Some("test-id")
                && f.get("client_secret").map(String::as_str) == Some("test-secret")
                && f.get("scope").map(String::as_str) == Some("vardefulla-datamangder:read");
            if !ok {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            Json(json!({ "access_token": "tok-123", "expires_in": 3600 })).into_response()
        }
        async fn fake_orgs(headers: HeaderMap, Json(body): Json<Value>) -> Response {
            if headers.get("authorization").and_then(|v| v.to_str().ok()) != Some("Bearer tok-123") {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            let raw = match body["identitetsbeteckning"].as_str() {
                Some("5299999994") => bolagsverket::fixtures::AKTIEBOLAG,
                Some("5560000019") => bolagsverket::fixtures::KALLA_OTILLGANGLIG,
                Some("5560000027") => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                _ => bolagsverket::fixtures::FINNS_EJ,
            };
            Json(serde_json::from_str::<Value>(raw).unwrap()).into_response()
        }

        let fake = Router::new()
            .route("/oauth2/token", post(fake_token))
            .route("/vardefulla-datamangder/v1/organisationer", post(fake_orgs));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, fake).await.unwrap() });

        std::env::set_var("BOLAGSVERKET_CLIENT_ID", "test-id");
        std::env::set_var("BOLAGSVERKET_CLIENT_SECRET", "test-secret");
        std::env::set_var("BOLAGSVERKET_BASE_URL", format!("http://{addr}/vardefulla-datamangder/v1"));

        let (status, _, body) = get_path("/foretag/5299999994").await;
        assert_eq!(status, StatusCode::OK);
        for needle in [
            "Cykelbolaget AB",
            "529999-9994",
            "Avregistrerad 2023-05-05",
            "Konkurs (sedan 2024-01-26)",
            "Jobbstigen 2, 12345 Grönköping",
            "Bedriva handel med cyklar och tillbehör till cyklar",
            "Bolagsverket, värdefulla datamängder (live)",
        ] {
            assert!(body.contains(needle), "la ficha real no contiene {needle:?}");
        }
        // Buscador: un organisationsnummer real que no es de ejemplo.
        let (_, _, sok) = get_path("/sok?q=529999-9994").await;
        assert!(sok.contains("Cykelbolaget AB") && sok.contains("Bolagsverket"));
        assert!(sok.contains("1 träff"));
        // No existe → 404; fuente de datos caída o error HTTP del API → 502 con página de error.
        assert_eq!(get_path("/foretag/5560000001").await.0, StatusCode::NOT_FOUND);
        // Dígito de control inválido: se rechaza antes de llamar a la API (404, no 502).
        assert_eq!(get_path("/foretag/5560000009").await.0, StatusCode::NOT_FOUND);
        assert_eq!(get_path("/foretag/5560000019").await.0, StatusCode::BAD_GATEWAY);
        assert_eq!(get_path("/foretag/5560000027").await.0, StatusCode::BAD_GATEWAY);
        // Un personnummer (12 dígitos) nunca se envía al API.
        assert_eq!(get_path("/foretag/194009272719").await.0, StatusCode::NOT_FOUND);
        // Las empresas de EJEMPLO siguen sirviéndose igual.
        assert_eq!(get_path("/foretag/559012-3456").await.0, StatusCode::OK);

        std::env::remove_var("BOLAGSVERKET_CLIENT_ID");
        std::env::remove_var("BOLAGSVERKET_CLIENT_SECRET");
        std::env::remove_var("BOLAGSVERKET_BASE_URL");
    }

    #[tokio::test]
    async fn interactive_elements_expose_tooltips() {
        // Gráfico de ingresos: 5 barras enfocables, cada una con su tooltip (y variación respecto al año anterior).
        let (_, _, co) = get_path("/foretag/559108-7721").await;
        assert_eq!(co.matches(r#"class="bar-group""#).count(), 5);
        assert!(co.contains("mot 2023"), "la barra de 2024 compara con 2023");
        assert!(co.contains(r#"data-copy="559108-7721""#), "botón de copiar el organisationsnummer");
        assert!(co.contains(r#"class="term""#) && co.contains("Soliditet"));
        assert!(co.contains(r#"class="info""#));
        // Cada barra enfocable lleva el mismo texto en aria-label y data-tip (accesible sin ratón).
        assert_eq!(co.matches(r#"tabindex="0""#).count(), 5 + 3,
            "5 barras + 3 pistas de comparación con el sector, todas alcanzables con Tab");
        // Gráfico de caja: 13 semanas + la línea de gráns.
        let (_, _, cash) = get_path("/likviditet").await;
        assert_eq!(cash.matches(r#"class="bar-group""#).count(), 13);
        assert!(cash.contains("under gränsen"));
        assert!(cash.contains(r#"class="threshold-group""#));
        // Buscador: cabeceras con pista de orden y atajo "/".
        let (_, _, sok) = get_path("/sok").await;
        assert!(sok.contains("Sortera stigande efter företag"));
        assert!(sok.contains("<kbd>/</kbd>"));
        // Los tooltips no deben duplicarse con title nativo.
        assert!(!sok.contains(" title="));
    }

    #[test]
    fn pending_benchmark_renders_skeleton_with_fragment_url() {
        let html = views::company_page(&model::EXAMPLE_COMPANIES[0], "ov", &views::Bench::Pending).into_string();
        assert!(html.contains(r#"data-fragment="/foretag/559012-3456/benchmarks""#));
        assert!(html.contains(r#"aria-busy="true""#));
        assert!(html.contains("skeleton-row"));
        assert!(html.contains("<noscript>"));
    }

    #[tokio::test]
    async fn serves_stylesheet() {
        let (status, _, body) = get_path("/static/styles.css").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("--accent: #0b6e75"));
    }
}
