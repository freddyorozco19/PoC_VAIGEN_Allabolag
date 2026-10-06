//! Manejadores de las rutas. Cada uno recibe el contexto de la petición que prepara `app::session_mw`.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Extension, Form};
use maud::Markup;
use serde::Deserialize;

use crate::app::{log_event, log_event_as, role_from_form, safe_next, AppState, ReqInfo};
use crate::auth::{self, PRE_CSRF_COOKIE, SESSION_COOKIE, SESSION_TTL_SECS};
use crate::db::{ActivityFilter, DbError, NewUser, Role, User};
use crate::i18n::Lang;
use crate::util;
use crate::views::{self, Ctx};
use crate::views_admin::{self, ActivityView, Notice, UserFormData};
use crate::handlers_tools as tools;
use crate::{annual_report, bolagsverket, model, scb, views_fin};

type Info = Extension<Arc<ReqInfo>>;

const STYLES: &str = include_str!("../static/styles.css");

pub fn html(markup: Markup) -> Response {
    Html(markup.into_string()).into_response()
}

pub fn html_status(status: StatusCode, markup: Markup) -> Response {
    (status, Html(markup.into_string())).into_response()
}

fn redirect_with_cookies(to: &str, cookies: &[String]) -> Response {
    let mut resp = Redirect::to(to).into_response();
    for c in cookies {
        if let Ok(v) = HeaderValue::from_str(c) {
            resp.headers_mut().append(header::SET_COOKIE, v);
        }
    }
    resp
}

pub(crate) fn forbidden(c: &Ctx) -> Response {
    html_status(StatusCode::FORBIDDEN, views::message_page(c, c.t("err.forbidden_title"), c.t("err.forbidden")))
}

fn bad_csrf(c: &Ctx) -> Response {
    html_status(StatusCode::FORBIDDEN, views::message_page(c, c.t("err.csrf_title"), c.t("err.csrf")))
}

// ───────────────────────── Páginas existentes ─────────────────────────

pub async fn root() -> Redirect {
    Redirect::temporary("/sok")
}

pub async fn healthz() -> &'static str {
    "ok"
}

/// Huella corta del CSS: va en la URL de la hoja de estilos para que un cambio se vea de inmediato
/// aunque el navegador la tenga en caché.
pub fn css_version() -> &'static str {
    static VERSION: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| util::sha256_hex(STYLES)[..10].to_string());
    &VERSION
}

pub async fn styles() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=86400")], STYLES)
}

#[derive(Deserialize)]
pub struct SokParams {
    q: Option<String>,
    sort: Option<String>,
    dir: Option<String>,
}

/// Cuántos resultados del registro se muestran como máximo (se pide uno más para saber si hay más).
const REGISTRY_LIMIT: usize = 25;

pub async fn sok(State(st): State<AppState>, Extension(info): Info, Query(p): Query<SokParams>) -> Response {
    let c = info.ctx();
    let q = p.q.as_deref().unwrap_or("");
    // Un organisationsnummer que no es de EJEMPLO se busca en Bolagsverket (si hay credenciales).
    // Este API solo consulta por número, no por nombre.
    let live = if (!c.demo || model::search_example_companies(q).is_empty()) && bolagsverket::configured() {
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
    // Búsqueda por nombre en el índice del registro (archivo oficial de Bolagsverket). Un número de organización
    // no se busca aquí: lo resuelve la API en vivo.
    let by_name = if live.is_none() && q.trim().chars().count() >= 2 && bolagsverket::normalize_org_number(q).is_none() {
        match st.registry.get() {
            Some(reg) => {
                let query = q.to_string();
                let mut hits = tokio::task::spawn_blocking(move || reg.search(&query, REGISTRY_LIMIT + 1, true)).await.unwrap_or_default();
                let more = hits.len() > REGISTRY_LIMIT;
                hits.truncate(REGISTRY_LIMIT);
                Some(views::RegistryHits { hits, more })
            }
            None => None,
        }
    } else {
        None
    };
    html(views::sok_page(&c, q, p.sort.as_deref(), p.dir.as_deref(), live.as_ref(), by_name.as_ref()))
}

#[derive(Deserialize)]
pub struct CompanyParams {
    tab: Option<String>,
}

/// Empresa que no es de EJEMPLO: ficha real de Bolagsverket si hay credenciales; si no, 404.
async fn live_company(st: &AppState, info: &ReqInfo, c: &Ctx, org: &str, tab: &str) -> Response {
    if !bolagsverket::configured() {
        return html_status(StatusCode::NOT_FOUND, views::company_not_found_page(c));
    }
    match bolagsverket::get_organisation_by_number(org).await {
        Ok(o) => {
            // Las cuentas anuales requieren descargar y leer hasta 3 informes (varios segundos): si no están
            // en caché, la ficha sale al instante con un esqueleto y el navegador pide `/foretag/:org/bokslut`.
            let fin = match tools::cached_inputs(&o, c.lang) {
                Some((f, m)) => {
                    tools::record_snapshot(st, &o, f.as_ref(), m.as_ref()); // mantiene al día "Mis empresas"
                    views::FinState::Ready(f, m)
                }
                None => views::FinState::Pending,
            };
            let following = info.user.as_ref().is_some_and(|u| st.db.watch_has(u.id, &o.organisationsnummer));
            html(views::live_profile_page(c, &o, &fin, following, tab))
        }
        Err(bolagsverket::BvError::NotFound(_) | bolagsverket::BvError::Invalid(_)) => {
            html_status(StatusCode::NOT_FOUND, views::company_not_found_page(c))
        }
        Err(e) => {
            eprintln!("{e}");
            // Límite de 60 peticiones/minuto superado → 429; cualquier otro fallo del API → 502.
            let status = match e {
                bolagsverket::BvError::Upstream { status: Some(429), .. } => StatusCode::TOO_MANY_REQUESTS,
                _ => StatusCode::BAD_GATEWAY,
            };
            html_status(status, views::live_error_page(c))
        }
    }
}

pub async fn company(State(st): State<AppState>, Extension(info): Info, Path(org): Path<String>, Query(p): Query<CompanyParams>) -> Response {
    let c = info.ctx();
    match model::find_example_company(&org).filter(|_| info.demo) {
        Some(co) => {
            // Medianas reales del sector (SCB). Si ya están en caché la ficha sale completa al instante;
            // si no, sale con un esqueleto y el navegador pide el fragmento.
            let bench = if !scb::enabled() {
                views::Bench::Example
            } else {
                match scb::peek(co.sni_code, co.employee_range, c.lang) {
                    Some(m) => views::Bench::Ready(m),
                    None => views::Bench::Pending,
                }
            };
            html(views::company_page(&c, co, p.tab.as_deref().unwrap_or("ov"), &bench))
        }
        None => live_company(&st, &info, &c, &org, p.tab.as_deref().unwrap_or("ov")).await,
    }
}

/// Fragmento HTML con la valoración y las cifras de las cuentas anuales de una empresa real (lo pide la ficha en
/// segundo plano).
pub async fn company_bokslut(State(st): State<AppState>, Extension(info): Info, Path(org): Path<String>, Query(p): Query<CompanyParams>) -> Response {
    let c = info.ctx();
    if (info.demo && model::find_example_company(&org).is_some()) || !bolagsverket::configured() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match tools::load_analysis(&org, c.lang).await {
        Ok(a) => {
            tools::record_snapshot(&st, &a.org, a.fin.as_ref(), a.medians.as_ref());
            // Sin cuentas: ¿pertenece a un grupo cuya matriz sí las tiene (ESEF)? Se averigua aquí para pintarlo en la ficha.
            if !a.fin.as_ref().is_some_and(|f| f.latest().is_some()) {
                if let Err(e) = crate::esef::group_of(&a.org.organisationsnummer).await {
                    eprintln!("ESEF grupo {}: {e}", a.org.organisationsnummer);
                }
            }
            // Los informes ya están guardados con todos sus hechos: las pestañas de personas y de datos salen de ahí.
            match p.tab.as_deref() {
                Some("fin") => html(views_fin::finance_tab(&c, &a.org, a.fin.as_ref(), a.medians.as_ref())),
                Some("ppl") => html(views_fin::people_tab(&c, &a.org, &annual_report::stored_reports(&a.org.organisationsnummer))),
                Some("dat") => html(views_fin::data_tab(&c, &a.org, &annual_report::stored_reports(&a.org.organisationsnummer))),
                _ => html(views::financials_fragment(&c, &a.org, a.fin.as_ref(), a.medians.as_ref())),
            }
        }
        Err(status) => status.into_response(),
    }
}

/// Fragmento HTML con la comparación con el sector (lo pide la ficha cuando SCB aún no estaba en caché).
pub async fn company_benchmarks(Extension(info): Info, Path(org): Path<String>) -> Response {
    let c = info.ctx();
    let Some(co) = model::find_example_company(&org).filter(|_| info.demo) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (medians, notice) = if scb::enabled() {
        match scb::get_sector_medians(co.sni_code, co.employee_range, c.lang).await {
            Ok(m) => (m, None),
            Err(e) => {
                eprintln!("{e}");
                (None, Some(c.t("ov.bench_failed")))
            }
        }
    } else {
        (None, None)
    };
    html(views::benchmark_fragment(&c, co, medians.as_ref(), notice))
}

pub async fn likviditet(Extension(info): Info) -> Response {
    if !info.demo {
        return html_status(StatusCode::NOT_FOUND, views::not_found_page(&info.ctx()));
    }
    html(views::likviditet_page(&info.ctx()))
}

pub async fn sie(Extension(info): Info) -> Response {
    if !info.demo {
        return html_status(StatusCode::NOT_FOUND, views::not_found_page(&info.ctx()));
    }
    html(views::sie_page(&info.ctx()))
}

pub async fn fakturor(Extension(info): Info) -> Response {
    if !info.demo {
        return html_status(StatusCode::NOT_FOUND, views::not_found_page(&info.ctx()));
    }
    html(views::fakturor_page(&info.ctx()))
}

pub async fn fallback(Extension(info): Info) -> Response {
    html_status(StatusCode::NOT_FOUND, views::not_found_page(&info.ctx()))
}

// ───────────────────────── Login / logout ─────────────────────────

#[derive(Deserialize)]
pub struct LoginQuery {
    next: Option<String>,
}

pub async fn login_get(State(st): State<AppState>, Extension(info): Info, Query(q): Query<LoginQuery>) -> Response {
    let next = safe_next(q.next.as_deref().unwrap_or(""));
    if info.user.is_some() {
        return Redirect::to(&next).into_response();
    }
    let pre = util::random_hex(16);
    let cookie = auth::set_cookie(PRE_CSRF_COOKIE, &pre, 3600, true, info.secure_cookie(&st));
    let mut resp = html(views_admin::login_page(&info.ctx(), "", &next, None, &pre));
    resp.headers_mut().append(header::SET_COOKIE, HeaderValue::from_str(&cookie).expect("cookie válida"));
    resp
}

#[derive(Deserialize)]
pub struct LoginForm {
    #[serde(default)]
    identifier: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    csrf: String,
    next: Option<String>,
}

pub async fn login_post(State(st): State<AppState>, Extension(info): Info, headers: axum::http::HeaderMap, Form(f): Form<LoginForm>) -> Response {
    let c = info.ctx();
    let next = safe_next(f.next.as_deref().unwrap_or(""));
    let ident = f.identifier.trim().to_string();
    let render = |status: StatusCode, err: &str| {
        let pre = util::random_hex(16);
        let cookie = auth::set_cookie(PRE_CSRF_COOKIE, &pre, 3600, true, info.secure_cookie(&st));
        let mut resp = html_status(status, views_admin::login_page(&c, &ident, &next, Some(err), &pre));
        resp.headers_mut().append(header::SET_COOKIE, HeaderValue::from_str(&cookie).expect("cookie válida"));
        resp
    };

    // CSRF de inicio de sesión: la cookie previa debe coincidir con el campo oculto.
    let cookie_hdr = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()).unwrap_or("");
    let pre = auth::cookie_value(cookie_hdr, PRE_CSRF_COOKIE).unwrap_or_default();
    if pre.is_empty() || !util::ct_eq(&pre, &f.csrf) {
        return render(StatusCode::FORBIDDEN, "auth.err.csrf");
    }

    let keys = [format!("ip:{}", info.ip), format!("id:{}", ident.to_lowercase())];
    if keys.iter().any(|k| st.limiter.blocked(k)) {
        log_event_as(&st, None, &info, "login_blocked", "POST", 429, &ident.chars().take(80).collect::<String>());
        return render(StatusCode::TOO_MANY_REQUESTS, "auth.err.blocked");
    }

    let found = st.db.user_by_login(&ident);
    let ok = match &found {
        Some((u, hash)) if u.active => auth::verify_password(hash, &f.password),
        Some(_) => {
            auth::dummy_verify(&f.password);
            false
        }
        None => {
            auth::dummy_verify(&f.password);
            false
        }
    };
    let Some((user, _)) = found.filter(|_| ok) else {
        for k in &keys {
            st.limiter.record_failure(k);
        }
        // Se registra el identificador probado (nunca la contraseña).
        log_event_as(&st, None, &info, "login_fail", "POST", 401, &ident.chars().take(80).collect::<String>());
        return render(StatusCode::UNAUTHORIZED, "auth.err.invalid");
    };

    for k in &keys {
        st.limiter.clear(k);
    }
    let (token, _csrf) = st.db.create_session(user.id, SESSION_TTL_SECS, &info.ip, &info.ua);
    st.db.touch_login(user.id);
    log_event_as(&st, Some(&user), &info, "login_ok", "POST", 303, "");
    let secure = info.secure_cookie(&st);
    let cookies = [
        auth::set_cookie(SESSION_COOKIE, &token, SESSION_TTL_SECS, true, secure),
        auth::set_cookie(auth::LANG_COOKIE, user.lang.code(), 365 * 86_400, false, secure),
        auth::set_cookie(PRE_CSRF_COOKIE, "", 0, true, secure),
    ];
    redirect_with_cookies(if user.must_change { "/profile?force=1" } else { &next }, &cookies)
}

#[derive(Deserialize)]
pub struct CsrfOnly {
    #[serde(default)]
    csrf: String,
}

pub async fn logout_post(State(st): State<AppState>, Extension(info): Info, Form(f): Form<CsrfOnly>) -> Response {
    if !info.csrf_ok(&f.csrf) {
        return bad_csrf(&info.ctx());
    }
    if let Some(t) = &info.session_token {
        st.db.delete_session(t);
    }
    log_event(&st, &info, "logout", "POST", 303, "");
    redirect_with_cookies("/login", &[auth::set_cookie(SESSION_COOKIE, "", 0, true, info.secure_cookie(&st))])
}

// ───────────────────────── Perfil ─────────────────────────

#[derive(Deserialize)]
pub struct ProfileQuery {
    force: Option<String>,
}

pub async fn profile_get(Extension(info): Info, Query(q): Query<ProfileQuery>) -> Response {
    let Some(user) = info.user.as_ref() else { return StatusCode::UNAUTHORIZED.into_response() };
    html(views_admin::profile_page(&info.ctx(), user, q.force.is_some() || user.must_change, None, None))
}

#[derive(Deserialize)]
pub struct ProfileForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    lang: String,
}

pub async fn profile_post(State(st): State<AppState>, Extension(info): Info, Form(f): Form<ProfileForm>) -> Response {
    let Some(user) = info.user.as_ref() else { return StatusCode::UNAUTHORIZED.into_response() };
    let c = info.ctx();
    if !info.csrf_ok(&f.csrf) {
        return bad_csrf(&c);
    }
    let name = f.name.trim();
    let lang = Lang::from_code(&f.lang).unwrap_or(user.lang);
    if name.is_empty() || name.chars().count() > 80 {
        let n = Notice { ok: false, text: c.t("usr.err.name") };
        return html_status(StatusCode::UNPROCESSABLE_ENTITY, views_admin::profile_page(&c, user, user.must_change, Some(&n), None));
    }
    let _ = st.db.update_profile(user.id, name, lang);
    log_event(&st, &info, "profile_update", "POST", 200, &format!("lang={}", lang.code()));
    let fresh = st.db.user_by_id(user.id).unwrap_or_else(|| user.clone());
    let mut c2 = c.clone();
    c2.lang = lang;
    c2.user = Some(fresh.clone());
    let n = Notice { ok: true, text: c2.t("prof.saved") };
    let mut resp = html(views_admin::profile_page(&c2, &fresh, fresh.must_change, Some(&n), None));
    let cookie = auth::set_cookie(auth::LANG_COOKIE, lang.code(), 365 * 86_400, false, info.secure_cookie(&st));
    resp.headers_mut().append(header::SET_COOKIE, HeaderValue::from_str(&cookie).expect("cookie válida"));
    resp
}

#[derive(Deserialize)]
pub struct PasswordForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    current: String,
    #[serde(default)]
    new1: String,
    #[serde(default)]
    new2: String,
}

pub async fn profile_password_post(State(st): State<AppState>, Extension(info): Info, Form(f): Form<PasswordForm>) -> Response {
    let Some(user) = info.user.as_ref() else { return StatusCode::UNAUTHORIZED.into_response() };
    let c = info.ctx();
    if !info.csrf_ok(&f.csrf) {
        return bad_csrf(&c);
    }
    let fail = |key: &str| {
        let n = Notice { ok: false, text: c.t(key) };
        html_status(StatusCode::UNPROCESSABLE_ENTITY, views_admin::profile_page(&c, user, user.must_change, None, Some(&n)))
    };
    let hash = st.db.hash_of(user.id).unwrap_or_default();
    if !auth::verify_password(&hash, &f.current) {
        log_event(&st, &info, "password_change_fail", "POST", 422, "");
        return fail("prof.err.current");
    }
    if f.new1 != f.new2 {
        return fail("prof.err.mismatch");
    }
    if let Some(problem) = auth::password_problem(&f.new1) {
        return fail(problem);
    }
    if f.new1 == f.current {
        return fail("prof.err.same");
    }
    let _ = st.db.set_password(user.id, &auth::hash_password(&f.new1), false);
    // Se cierran todas las sesiones (también las de otros dispositivos) y se abre una nueva para esta.
    st.db.delete_user_sessions(user.id);
    let (token, _) = st.db.create_session(user.id, SESSION_TTL_SECS, &info.ip, &info.ua);
    log_event(&st, &info, "password_change", "POST", 303, "");
    redirect_with_cookies("/profile?saved=1", &[auth::set_cookie(SESSION_COOKIE, &token, SESSION_TTL_SECS, true, info.secure_cookie(&st))])
}

// ───────────────────────── Usuarios (CRUD) ─────────────────────────

fn actor_for_admin(info: &ReqInfo) -> Option<User> {
    info.user.clone().filter(auth::can_admin_users)
}

fn valid_email(e: &str) -> bool {
    let e = e.trim();
    e.len() <= 120 && !e.contains(char::is_whitespace) && e.split_once('@').is_some_and(|(l, d)| !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.'))
}

fn valid_username(u: &str) -> bool {
    (3..=40).contains(&u.len()) && u.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[derive(Deserialize)]
pub struct UsersQuery {
    q: Option<String>,
    role: Option<String>,
    ok: Option<String>,
}

pub async fn users_list(State(st): State<AppState>, Extension(info): Info, Query(p): Query<UsersQuery>) -> Response {
    let c = info.ctx();
    let Some(actor) = actor_for_admin(&info) else { return forbidden(&c) };
    let q = p.q.clone().unwrap_or_default();
    let role = p.role.as_deref().and_then(Role::from_code);
    let users = st.db.list_users(&q, role);
    html(views_admin::users_page(&c, &actor, &users, &q, role, p.ok.as_deref(), st.db.count_active_superadmins()))
}

pub async fn user_new_get(Extension(info): Info) -> Response {
    let c = info.ctx();
    let Some(actor) = actor_for_admin(&info) else { return forbidden(&c) };
    let data = UserFormData { role: Role::User.code().into(), active: true, lang: c.lang.code().into(), must_change: true, ..Default::default() };
    html(views_admin::user_form_page(&c, &actor, None, &data, None))
}

#[derive(Deserialize, Default)]
pub struct UserForm {
    #[serde(default)]
    csrf: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    lang: String,
    #[serde(default)]
    active: Option<String>,
    #[serde(default)]
    must_change: Option<String>,
    #[serde(default)]
    password: String,
}

impl UserForm {
    fn data(&self) -> UserFormData {
        UserFormData {
            name: self.name.clone(),
            email: self.email.clone(),
            username: self.username.clone(),
            role: self.role.clone(),
            active: self.active.is_some(),
            lang: self.lang.clone(),
            must_change: self.must_change.is_some(),
            password: self.password.clone(),
        }
    }
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

pub async fn user_create(State(st): State<AppState>, Extension(info): Info, Form(f): Form<UserForm>) -> Response {
    let c = info.ctx();
    let Some(actor) = actor_for_admin(&info) else { return forbidden(&c) };
    if !info.csrf_ok(&f.csrf) {
        return bad_csrf(&c);
    }
    let data = f.data();
    let show_error = |key: &str| html_status(StatusCode::UNPROCESSABLE_ENTITY, views_admin::user_form_page(&c, &actor, None, &data, Some(key)));

    let name = f.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return show_error("usr.err.name");
    }
    let email = non_empty(&f.email);
    let username = if actor.role == Role::Superadmin { non_empty(&f.username) } else { None };
    if email.is_none() && username.is_none() {
        return show_error("usr.err.login_required");
    }
    if email.as_deref().is_some_and(|e| !valid_email(e)) {
        return show_error("usr.err.email");
    }
    if username.as_deref().is_some_and(|u| !valid_username(u)) {
        return show_error("usr.err.username");
    }
    let Some(role) = role_from_form(&f.role).filter(|r| auth::assignable_roles(&actor).contains(r)) else {
        return show_error("usr.err.role_not_allowed");
    };
    let lang = Lang::from_code(&f.lang).unwrap_or(Lang::DEFAULT);
    let provided = non_empty(&f.password);
    if let Some(p) = &provided {
        if let Some(problem) = auth::password_problem(p) {
            return show_error(problem);
        }
    }
    let generated = provided.is_none();
    let password = provided.unwrap_or_else(|| util::gen_password(14));
    let new = NewUser {
        email: email.clone(),
        username: username.clone(),
        name: name.to_string(),
        role,
        active: true,
        lang,
        pass_hash: auth::hash_password(&password),
        must_change: f.must_change.is_some() || generated,
        created_by: Some(actor.id),
    };
    match st.db.create_user(&new) {
        Ok(id) => {
            log_event(&st, &info, "user_create", "POST", 200, &format!("{} · {}", email.or(username).unwrap_or_default(), role.code()));
            let target = st.db.user_by_id(id).expect("recién creado");
            if generated {
                html(views_admin::user_password_page(&c, &target, &password, true))
            } else {
                redirect_with_cookies("/users?ok=created", &[])
            }
        }
        Err(DbError::Duplicate("email")) => show_error("usr.err.dup_email"),
        Err(DbError::Duplicate(_)) => show_error("usr.err.dup_username"),
        Err(DbError::Other(e)) => {
            eprintln!("{e}");
            show_error("err.generic")
        }
    }
}

fn load_manageable(st: &AppState, actor: &User, id: i64) -> Result<User, StatusCode> {
    let target = st.db.user_by_id(id).ok_or(StatusCode::NOT_FOUND)?;
    if auth::can_manage(actor, &target) { Ok(target) } else { Err(StatusCode::FORBIDDEN) }
}

fn denied(c: &Ctx, code: StatusCode) -> Response {
    if code == StatusCode::NOT_FOUND {
        html_status(StatusCode::NOT_FOUND, views::not_found_page(c))
    } else {
        forbidden(c)
    }
}

fn form_from_user(u: &User) -> UserFormData {
    UserFormData {
        name: u.name.clone(),
        email: u.email.clone().unwrap_or_default(),
        username: u.username.clone().unwrap_or_default(),
        role: u.role.code().into(),
        active: u.active,
        lang: u.lang.code().into(),
        must_change: false,
        password: String::new(),
    }
}

pub async fn user_edit_get(State(st): State<AppState>, Extension(info): Info, Path(id): Path<i64>) -> Response {
    let c = info.ctx();
    let Some(actor) = actor_for_admin(&info) else { return forbidden(&c) };
    match load_manageable(&st, &actor, id) {
        Ok(t) => html(views_admin::user_form_page(&c, &actor, Some(&t), &form_from_user(&t), None)),
        Err(code) => denied(&c, code),
    }
}

pub async fn user_update(State(st): State<AppState>, Extension(info): Info, Path(id): Path<i64>, Form(f): Form<UserForm>) -> Response {
    let c = info.ctx();
    let Some(actor) = actor_for_admin(&info) else { return forbidden(&c) };
    if !info.csrf_ok(&f.csrf) {
        return bad_csrf(&c);
    }
    let target = match load_manageable(&st, &actor, id) {
        Ok(t) => t,
        Err(code) => return denied(&c, code),
    };
    let data = f.data();
    let show_error = |key: &str| html_status(StatusCode::UNPROCESSABLE_ENTITY, views_admin::user_form_page(&c, &actor, Some(&target), &data, Some(key)));

    let name = f.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return show_error("usr.err.name");
    }
    let email = non_empty(&f.email);
    // Solo el superadmin cambia el nombre de usuario.
    let username = if actor.role == Role::Superadmin { non_empty(&f.username) } else { target.username.clone() };
    if email.is_none() && username.is_none() {
        return show_error("usr.err.login_required");
    }
    if email.as_deref().is_some_and(|e| !valid_email(e)) {
        return show_error("usr.err.email");
    }
    if username.as_deref().is_some_and(|u| !valid_username(u)) {
        return show_error("usr.err.username");
    }
    let Some(role) = role_from_form(&f.role) else { return show_error("usr.err.role_not_allowed") };
    let active = f.active.is_some();
    if let Some(problem) = auth::forbidden_change(&actor, &target, role, active, st.db.count_active_superadmins()) {
        return show_error(problem);
    }
    let lang = Lang::from_code(&f.lang).unwrap_or(target.lang);
    match st.db.update_user(id, name, email.as_deref(), username.as_deref(), role, active, lang) {
        Ok(()) => {
            // Un cambio de rol o de estado cierra sus sesiones: los permisos nuevos rigen desde ya.
            if role != target.role || active != target.active {
                st.db.delete_user_sessions(id);
            }
            let mut changes: Vec<String> = Vec::new();
            if role != target.role {
                changes.push(format!("role {}→{}", target.role.code(), role.code()));
            }
            if active != target.active {
                changes.push(format!("active {}→{}", target.active, active));
            }
            log_event(&st, &info, "user_update", "POST", 303, &format!("{} {}", target.label(), changes.join(", ")).trim().to_string());
            redirect_with_cookies("/users?ok=updated", &[])
        }
        Err(DbError::Duplicate("email")) => show_error("usr.err.dup_email"),
        Err(DbError::Duplicate(_)) => show_error("usr.err.dup_username"),
        Err(DbError::Other(e)) => {
            eprintln!("{e}");
            show_error("err.generic")
        }
    }
}

pub async fn user_delete_get(State(st): State<AppState>, Extension(info): Info, Path(id): Path<i64>) -> Response {
    let c = info.ctx();
    let Some(actor) = actor_for_admin(&info) else { return forbidden(&c) };
    match load_manageable(&st, &actor, id) {
        Ok(t) if auth::can_delete(&actor, &t, st.db.count_active_superadmins()) => html(views_admin::user_delete_page(&c, &t)),
        Ok(_) => forbidden(&c),
        Err(code) => denied(&c, code),
    }
}

pub async fn user_delete_post(State(st): State<AppState>, Extension(info): Info, Path(id): Path<i64>, Form(f): Form<CsrfOnly>) -> Response {
    let c = info.ctx();
    let Some(actor) = actor_for_admin(&info) else { return forbidden(&c) };
    if !info.csrf_ok(&f.csrf) {
        return bad_csrf(&c);
    }
    let target = match load_manageable(&st, &actor, id) {
        Ok(t) => t,
        Err(code) => return denied(&c, code),
    };
    if !auth::can_delete(&actor, &target, st.db.count_active_superadmins()) {
        return forbidden(&c);
    }
    match st.db.delete_user(id) {
        Ok(_) => {
            log_event(&st, &info, "user_delete", "POST", 303, &format!("{} · {}", target.label(), target.role.code()));
            redirect_with_cookies("/users?ok=deleted", &[])
        }
        Err(e) => {
            eprintln!("{e:?}");
            html_status(StatusCode::INTERNAL_SERVER_ERROR, views::message_page(&c, c.t("err.generic_title"), c.t("err.generic")))
        }
    }
}

pub async fn user_reset_post(State(st): State<AppState>, Extension(info): Info, Path(id): Path<i64>, Form(f): Form<CsrfOnly>) -> Response {
    let c = info.ctx();
    let Some(actor) = actor_for_admin(&info) else { return forbidden(&c) };
    if !info.csrf_ok(&f.csrf) {
        return bad_csrf(&c);
    }
    let target = match load_manageable(&st, &actor, id) {
        Ok(t) if t.id != actor.id => t,
        Ok(_) => return forbidden(&c),
        Err(code) => return denied(&c, code),
    };
    let password = util::gen_password(14);
    if st.db.set_password(id, &auth::hash_password(&password), true).is_err() {
        return html_status(StatusCode::INTERNAL_SERVER_ERROR, views::message_page(&c, c.t("err.generic_title"), c.t("err.generic")));
    }
    st.db.delete_user_sessions(id);
    log_event(&st, &info, "password_reset", "POST", 200, &target.label());
    let target = st.db.user_by_id(id).unwrap_or(target);
    html(views_admin::user_password_page(&c, &target, &password, false))
}

// ───────────────────────── Actividad ─────────────────────────

#[derive(Deserialize, Default)]
pub struct ActivityQuery {
    user: Option<String>,
    event: Option<String>,
    q: Option<String>,
    from: Option<String>,
    to: Option<String>,
    page: Option<i64>,
}

fn valid_date(d: &str) -> bool {
    d.len() == 10 && d.chars().enumerate().all(|(i, ch)| if i == 4 || i == 7 { ch == '-' } else { ch.is_ascii_digit() })
}

impl ActivityQuery {
    fn filter(&self) -> ActivityFilter {
        ActivityFilter {
            user_id: self.user.as_deref().and_then(|u| u.parse().ok()),
            event: self.event.clone().filter(|e| !e.is_empty() && e.len() <= 40),
            q: self.q.clone().filter(|q| !q.trim().is_empty()).map(|q| q.chars().take(100).collect()),
            from: self.from.clone().filter(|d| valid_date(d)),
            to: self.to.clone().filter(|d| valid_date(d)),
        }
    }
}

const ACTIVITY_PER_PAGE: i64 = 50;

pub async fn activity_get(State(st): State<AppState>, Extension(info): Info, Query(p): Query<ActivityQuery>) -> Response {
    let c = info.ctx();
    if !info.user.as_ref().is_some_and(auth::can_view_activity) {
        return forbidden(&c);
    }
    let filter = p.filter();
    let page = p.page.unwrap_or(1).max(1);
    let (rows, total) = st.db.activity(&filter, ACTIVITY_PER_PAGE, (page - 1) * ACTIVITY_PER_PAGE);
    let kpis = st.db.kpis();
    let summaries = st.db.user_summaries();
    let events = st.db.distinct_events();
    let users = st.db.list_users("", None);
    html(views_admin::activity_page(
        &c,
        &ActivityView { filter: &filter, rows: &rows, total, page, per_page: ACTIVITY_PER_PAGE, kpis: &kpis, summaries: &summaries, events: &events, users: &users },
    ))
}

/// Neutraliza fórmulas (=, +, -, @) al abrir el CSV en una hoja de cálculo y escapa las comillas.
pub(crate) fn csv_cell(s: &str) -> String {
    let safe = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{s}") } else { s.to_string() };
    format!("\"{}\"", safe.replace('"', "\"\""))
}

pub async fn activity_csv(State(st): State<AppState>, Extension(info): Info, Query(p): Query<ActivityQuery>) -> Response {
    let c = info.ctx();
    if !info.user.as_ref().is_some_and(auth::can_view_activity) {
        return forbidden(&c);
    }
    let filter = p.filter();
    let (rows, total) = st.db.activity(&filter, 5000, 0);
    log_event(&st, &info, "export", "GET", 200, &format!("{} filas", rows.len().min(total as usize)));
    let mut out = String::from("\u{feff}time_utc,user,role,event,method,path,query,status,ip,detail,user_agent\r\n");
    for r in &rows {
        let status = r.status.to_string();
        let cells: [&str; 11] = [&r.ts, &r.user_label, r.role.as_deref().unwrap_or(""), &r.event, &r.method, &r.path, &r.query, &status, &r.ip, &r.detail, &r.ua];
        out.push_str(&cells.iter().map(|s| csv_cell(s)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    (
        [(header::CONTENT_TYPE, "text/csv; charset=utf-8"), (header::CONTENT_DISPOSITION, "attachment; filename=\"siffra-actividad.csv\"")],
        out,
    )
        .into_response()
}
