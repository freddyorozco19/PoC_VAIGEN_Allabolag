//! Valoración de empresas reales (carga de datos compartida) y las pantallas de trabajo: Mis empresas,
//! Comparar e Historial.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Extension, Form};
use maud::Markup;
use serde::Deserialize;

use crate::analysis;
use crate::annual_report::{self, Financials};
use crate::app::{log_event, safe_next, AppState, ReqInfo};
use crate::bolagsverket::{self, BvError, Organisation};
use crate::db::{WatchSnapshot, WATCH_LIMIT};
use crate::i18n::Lang;
use crate::scb::{self, SectorMedians};
use crate::util;
use crate::views::{self, Ctx};
use crate::views_tools::{self, CompareCol, CompareData, Flash, SeenCompany};

type Info = Extension<Arc<ReqInfo>>;

/// Cuántas empresas se revisan como máximo con "Actualizar todas" (cada una puede tardar varios segundos).
const REFRESH_BATCH: usize = 8;

fn html(markup: Markup) -> Response {
    Html(markup.into_string()).into_response()
}

// ───────────────────────── Carga y registro de la valoración ─────────────────────────

/// Código SNI principal de la empresa (el primero que declara), si lo hay y SCB lo entiende.
pub fn main_sni(o: &Organisation) -> Option<&str> {
    o.sni.first().map(|(code, _)| code.as_str()).filter(|code| !scb::sni_candidates(code).is_empty())
}

/// Cuentas y medianas del sector ya en caché. `None` si falta alguna: la ficha sale entonces con un esqueleto y
/// el navegador pide `/foretag/:org/bokslut`, que las busca (descargar las cuentas tarda varios segundos).
pub fn cached_inputs(o: &Organisation, lang: Lang) -> Option<(Option<Financials>, Option<SectorMedians>)> {
    let fin = annual_report::peek(&o.organisationsnummer)?;
    let medians = match main_sni(o) {
        Some(code) if scb::enabled() => scb::peek(code, "", lang)?,
        _ => None,
    };
    Some((fin, medians))
}

/// Todo lo necesario para valorar una empresa.
pub struct Loaded {
    pub org: Organisation,
    pub fin: Option<Financials>,
    pub medians: Option<SectorMedians>,
}

fn bv_status(e: &BvError) -> StatusCode {
    match e {
        BvError::NotFound(_) | BvError::Invalid(_) => StatusCode::NOT_FOUND,
        BvError::Upstream { status: Some(429), .. } => StatusCode::TOO_MANY_REQUESTS,
        _ => StatusCode::BAD_GATEWAY,
    }
}

/// Registro (nombre, SNI, procedimientos) y cuentas anuales a la vez; después, la mediana del sector (SCB).
pub async fn load_analysis(orgnr: &str, lang: Lang) -> Result<Loaded, StatusCode> {
    let (org_res, fin_res) = tokio::join!(bolagsverket::get_organisation_by_number(orgnr), annual_report::get_financials(orgnr));
    let org = org_res.map_err(|e| {
        eprintln!("{e}"); // visible en el registro: un rechazo silencioso ocultó un fallo real
        bv_status(&e)
    })?;
    let fin = fin_res.map_err(|e| {
        eprintln!("{e}");
        bv_status(&e)
    })?;
    let medians = match main_sni(&org) {
        Some(code) if scb::enabled() => scb::get_sector_medians(code, "", lang).await.unwrap_or_else(|e| {
            eprintln!("{e}");
            None
        }),
        _ => None,
    };
    Ok(Loaded { org, fin, medians })
}

/// Anota la valoración actual de la empresa en "Mis empresas" de quien la sigue (y detecta cambios).
pub fn record_snapshot(st: &AppState, org: &Organisation, fin: Option<&Financials>, medians: Option<&SectorMedians>) {
    let today = util::now_iso();
    let s = analysis::snapshot(org, fin, medians, &today[..10]);
    st.db.watch_apply(
        &org.organisationsnummer,
        &WatchSnapshot {
            name: org.namn.clone(),
            form: org.organisationsform.clone(),
            level: analysis::level_code(s.level).to_string(),
            year: s.year,
            revenue: s.revenue,
            result: s.result,
            solidity: s.solidity,
            flags: s.flags,
        },
    );
}

// ───────────────────────── Mis empresas ─────────────────────────

#[derive(Deserialize, Default)]
pub struct WatchQuery {
    ok: Option<String>,
    err: Option<String>,
    n: Option<String>,
}

pub async fn watch_get(State(st): State<AppState>, Extension(info): Info, Query(q): Query<WatchQuery>) -> Response {
    let Some(user) = info.user.as_ref() else { return StatusCode::UNAUTHORIZED.into_response() };
    let c = info.ctx();
    let text: Option<(bool, String)> = match (q.ok.as_deref(), q.err.as_deref()) {
        (Some("added"), _) => Some((true, c.t("my.flash.added").to_string())),
        (Some("removed"), _) => Some((true, c.t("my.flash.removed").to_string())),
        (Some("refreshed"), _) => Some((true, c.tf("my.flash.refreshed", &[q.n.as_deref().unwrap_or("0")]))),
        (_, Some("limit")) => Some((false, c.tf("my.err.limit", &[&WATCH_LIMIT.to_string()]))),
        (_, Some("refresh")) => Some((false, c.t("my.err.refresh").to_string())),
        _ => None,
    };
    let flash = text.as_ref().map(|(ok, t)| Flash { ok: *ok, text: t });
    html(views_tools::watch_page(&c, &st.db.watch_list(user.id), flash.as_ref()))
}

#[derive(Deserialize)]
pub struct WatchForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    orgnr: String,
    next: Option<String>,
}

fn csrf_failed(c: &Ctx) -> Response {
    (StatusCode::FORBIDDEN, Html(views::message_page(c, c.t("err.csrf_title"), c.t("err.csrf")).into_string())).into_response()
}

fn next_of(f: &WatchForm) -> String {
    match f.next.as_deref().filter(|n| !n.is_empty()) {
        Some(n) => safe_next(n),
        None => "/bevakning".to_string(),
    }
}

pub async fn watch_add_post(State(st): State<AppState>, Extension(info): Info, Form(f): Form<WatchForm>) -> Response {
    let Some(user) = info.user.as_ref() else { return StatusCode::UNAUTHORIZED.into_response() };
    let c = info.ctx();
    if !info.csrf_ok(&f.csrf) {
        return csrf_failed(&c);
    }
    let Some(orgnr) = bolagsverket::normalize_org_number(&f.orgnr) else {
        return (StatusCode::NOT_FOUND, Html(views::company_not_found_page(&c).into_string())).into_response();
    };
    let next = next_of(&f);
    if st.db.watch_has(user.id, &orgnr) {
        return Redirect::to(&next).into_response();
    }
    if st.db.watch_count(user.id) >= WATCH_LIMIT {
        return Redirect::to("/bevakning?err=limit").into_response();
    }
    // El nombre sale del registro (en caché si se acaba de abrir la ficha); sin él no se puede guardar.
    let org = match bolagsverket::get_organisation_by_number(&orgnr).await {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return (bv_status(&e), Html(views::live_error_page(&c).into_string())).into_response();
        }
    };
    st.db.watch_add(user.id, &orgnr, &org.namn, &org.organisationsform);
    // Si las cuentas ya están en caché (se acaba de ver la ficha) la fila sale con su valoración.
    if let Some((fin, medians)) = cached_inputs(&org, c.lang) {
        record_snapshot(&st, &org, fin.as_ref(), medians.as_ref());
    }
    log_event(&st, &info, "watch_add", "POST", 303, &orgnr);
    Redirect::to(&next).into_response()
}

pub async fn watch_remove_post(State(st): State<AppState>, Extension(info): Info, Form(f): Form<WatchForm>) -> Response {
    let Some(user) = info.user.as_ref() else { return StatusCode::UNAUTHORIZED.into_response() };
    let c = info.ctx();
    if !info.csrf_ok(&f.csrf) {
        return csrf_failed(&c);
    }
    let orgnr = bolagsverket::normalize_org_number(&f.orgnr).unwrap_or_else(|| f.orgnr.clone());
    if st.db.watch_remove(user.id, &orgnr) {
        log_event(&st, &info, "watch_remove", "POST", 303, &orgnr);
    }
    let next = next_of(&f);
    if next == "/bevakning" { Redirect::to("/bevakning?ok=removed").into_response() } else { Redirect::to(&next).into_response() }
}

pub async fn watch_refresh_post(State(st): State<AppState>, Extension(info): Info, Form(f): Form<WatchForm>) -> Response {
    let Some(user) = info.user.as_ref() else { return StatusCode::UNAUTHORIZED.into_response() };
    let c = info.ctx();
    if !info.csrf_ok(&f.csrf) {
        return csrf_failed(&c);
    }
    let mut rows = st.db.watch_list(user.id);
    let targets: Vec<String> = if f.orgnr == "all" {
        // Las que hace más tiempo que no se revisan (o nunca), primero.
        rows.sort_by(|a, b| a.checked_at.cmp(&b.checked_at));
        rows.into_iter().take(REFRESH_BATCH).map(|r| r.orgnr).collect()
    } else {
        rows.into_iter().filter(|r| r.orgnr == f.orgnr).map(|r| r.orgnr).collect()
    };
    let mut done = 0;
    for orgnr in &targets {
        match load_analysis(orgnr, c.lang).await {
            Ok(a) => {
                record_snapshot(&st, &a.org, a.fin.as_ref(), a.medians.as_ref());
                done += 1;
            }
            Err(status) => eprintln!("Mis empresas: no se pudo actualizar {orgnr} ({status})"),
        }
    }
    log_event(&st, &info, "watch_refresh", "POST", 303, &format!("{done}/{}", targets.len()));
    if done == 0 && !targets.is_empty() {
        Redirect::to("/bevakning?err=refresh").into_response()
    } else {
        Redirect::to(&format!("/bevakning?ok=refreshed&n={done}")).into_response()
    }
}

// ───────────────────────── Comparar ─────────────────────────

pub async fn compare_get(State(st): State<AppState>, Extension(info): Info, Query(q): Query<Vec<(String, String)>>) -> Response {
    let c = info.ctx();
    let mut inputs: Vec<String> = q.iter().filter(|(k, _)| k == "o").map(|(_, v)| v.trim().chars().take(20).collect()).collect();
    inputs.truncate(4);
    while inputs.len() < 4 {
        inputs.push(String::new());
    }
    let mut orgs: Vec<String> = Vec::new();
    let mut invalid: Vec<String> = Vec::new();
    for v in inputs.iter().filter(|v| !v.is_empty()) {
        match bolagsverket::normalize_org_number(v) {
            Some(n) => {
                if !orgs.contains(&n) {
                    orgs.push(n);
                }
            }
            None => invalid.push(v.clone()),
        }
    }
    let notice = if !invalid.is_empty() {
        Some(c.tf("cmp.invalid", &[&invalid.join(", ")]))
    } else if orgs.len() == 1 {
        Some(c.t("cmp.need_two").to_string())
    } else {
        None
    };

    let mut cols: Vec<CompareCol> = Vec::new();
    if orgs.len() >= 2 && bolagsverket::configured() {
        // Las cuentas de cada empresa se piden a la vez (cada una puede tardar varios segundos).
        let lang = c.lang;
        let tasks: Vec<_> = orgs
            .iter()
            .map(|o| {
                let o = o.clone();
                tokio::spawn(async move { load_analysis(&o, lang).await })
            })
            .collect();
        let today = util::now_iso();
        for (orgnr, task) in orgs.iter().zip(tasks) {
            let data = match task.await {
                Ok(Ok(a)) => {
                    record_snapshot(&st, &a.org, a.fin.as_ref(), a.medians.as_ref());
                    let snap = analysis::snapshot(&a.org, a.fin.as_ref(), a.medians.as_ref(), &today[..10]);
                    Some(CompareData { org: a.org, snap, medians: a.medians })
                }
                _ => None,
            };
            cols.push(CompareCol { orgnr: orgnr.clone(), data });
        }
    }
    html(views_tools::compare_page(&c, &inputs, &cols, notice.as_deref()))
}

// ───────────────────────── Historial ─────────────────────────

pub async fn history_get(State(st): State<AppState>, Extension(info): Info) -> Response {
    let Some(user) = info.user.as_ref() else { return StatusCode::UNAUTHORIZED.into_response() };
    let known: HashMap<String, String> = st.db.watch_list(user.id).into_iter().map(|r| (r.orgnr, r.name)).collect();
    let registry = st.registry.get();
    let companies: Vec<SeenCompany> = st
        .db
        .recent_companies(user.id, 20)
        .into_iter()
        .map(|(orgnr, when)| {
            let name = known.get(&orgnr).cloned().or_else(|| registry.as_ref().and_then(|r| r.name_of(&orgnr)));
            SeenCompany { orgnr, name, when }
        })
        .collect();
    html(views_tools::history_page(&info.ctx(), &companies, &st.db.recent_searches(user.id, 15)))
}
