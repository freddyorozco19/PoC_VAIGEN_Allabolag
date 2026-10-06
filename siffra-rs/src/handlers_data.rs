//! Pantalla "Datos" (`/datos`) y su exportación (`/datos.csv`): estructura de las fuentes y dataset guardado.
//! Solo administradores y superadmin.

use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Extension;
use serde::Deserialize;

use crate::annual_report::mapped_members;
use crate::app::{log_event, AppState, ReqInfo};
use crate::auth;
use crate::handlers::{csv_cell, forbidden, html};
use crate::registry::Stats as RegistryStats;
use crate::views_data::{self, DataView};

type Info = Extension<Arc<ReqInfo>>;

const PER_PAGE: i64 = 50;
/// Máximo de conceptos que se exportan (hay del orden de miles).
const EXPORT_MAX: i64 = 50_000;
/// Las cifras del índice de nombres cuentan millones de filas: se calculan como mucho cada 10 minutos.
const REGISTRY_STATS_TTL: Duration = Duration::from_secs(600);

#[derive(Deserialize, Default)]
pub struct DataQuery {
    q: Option<String>,
    tax: Option<String>,
    page: Option<i64>,
}

impl DataQuery {
    fn q(&self) -> String {
        self.q.as_deref().unwrap_or("").trim().chars().take(80).collect()
    }
    /// Prefijo de taxonomía: solo letras, cifras, `-` y `_` (va a una consulta, y a la URL de las páginas).
    fn taxonomy(&self) -> String {
        self.tax.as_deref().unwrap_or("").trim().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(40).collect()
    }
}

type RegistryCache = Option<(usize, Instant, RegistryStats)>;
static REGISTRY_STATS: LazyLock<Mutex<RegistryCache>> = LazyLock::new(|| Mutex::new(None));

async fn registry_stats(st: &AppState) -> Option<RegistryStats> {
    let reg = st.registry.get()?;
    let key = Arc::as_ptr(&reg) as usize;
    if let Some((k, at, stats)) = REGISTRY_STATS.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if *k == key && at.elapsed() < REGISTRY_STATS_TTL {
            return Some(stats.clone());
        }
    }
    let stats = tokio::task::spawn_blocking(move || reg.stats()).await.ok()?;
    *REGISTRY_STATS.lock().unwrap_or_else(|e| e.into_inner()) = Some((key, Instant::now(), stats.clone()));
    Some(stats)
}

pub async fn datos_get(State(st): State<AppState>, Extension(info): Info, Query(p): Query<DataQuery>) -> Response {
    let c = info.ctx();
    if !info.user.as_ref().is_some_and(auth::can_admin_users) {
        return forbidden(&c);
    }
    let (q, taxonomy) = (p.q(), p.taxonomy());
    let page = p.page.unwrap_or(1).max(1);
    let db = st.db.clone();
    let (stats, (concepts, total)) = {
        let (db1, q1, t1) = (db.clone(), q.clone(), taxonomy.clone());
        // Consultas que recorren todos los hechos: fuera del hilo del servidor.
        let stats = tokio::task::spawn_blocking({
            let db = db.clone();
            move || db.data_stats()
        });
        let concepts = tokio::task::spawn_blocking(move || db1.concept_catalog(&q1, &t1, PER_PAGE, (page - 1) * PER_PAGE));
        (stats.await.unwrap_or_default(), concepts.await.unwrap_or_default())
    };
    let registry = registry_stats(&st).await;
    html(views_data::data_page(
        &c,
        &DataView { stats: &stats, registry: registry.as_ref(), concepts: &concepts, total_concepts: total, q: &q, taxonomy: &taxonomy, page, per_page: PER_PAGE },
    ))
}

pub async fn datos_csv(State(st): State<AppState>, Extension(info): Info, Query(p): Query<DataQuery>) -> Response {
    let c = info.ctx();
    if !info.user.as_ref().is_some_and(auth::can_admin_users) {
        return forbidden(&c);
    }
    let (q, taxonomy) = (p.q(), p.taxonomy());
    let db = st.db.clone();
    let (rows, total) = tokio::task::spawn_blocking(move || db.concept_catalog(&q, &taxonomy, EXPORT_MAX, 0)).await.unwrap_or_default();
    log_event(&st, &info, "export", "GET", 200, &format!("{} conceptos", rows.len().min(total as usize)));
    let mut out = String::from("\u{feff}concept,taxonomy,facts,reports,companies,numeric_facts,facts_with_breakdown,example,example_unit,example_period,siffra_fields\r\n");
    for r in &rows {
        let taxonomy = r.concept.split_once(':').map(|(t, _)| t).unwrap_or("");
        let example = match (r.example_value, &r.example_text) {
            (Some(v), _) => v.to_string(),
            (None, Some(t)) => t.clone(),
            _ => String::new(),
        };
        let cells: [String; 11] = [
            r.concept.clone(),
            taxonomy.to_string(),
            r.facts.to_string(),
            r.reports.to_string(),
            r.companies.to_string(),
            r.numeric.to_string(),
            r.with_dims.to_string(),
            example,
            r.example_unit.clone().unwrap_or_default(),
            r.example_period.clone(),
            mapped_members(&r.concept).join(" "),
        ];
        out.push_str(&cells.iter().map(|s| csv_cell(s)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    ([(header::CONTENT_TYPE, "text/csv; charset=utf-8"), (header::CONTENT_DISPOSITION, "attachment; filename=\"siffra-conceptos.csv\"")], out).into_response()
}
