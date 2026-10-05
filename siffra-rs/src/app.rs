//! Estado compartido y middleware: sesión, idioma, acceso obligatorio, cabeceras de seguridad y
//! registro de actividad. Todo lo que pasa por la aplicación pasa por `session_mw`.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};

use crate::auth::{self, Limiter, LANG_COOKIE, SESSION_COOKIE};
use crate::bolagsverket;
use crate::db::{Db, NewActivity, Role, User};
use crate::i18n::{self, Lang};
use crate::util::{self, query_param, urlencode};
use crate::views::Ctx;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub limiter: Arc<Limiter>,
    /// Fuerza la marca `Secure` en las cookies (producción detrás de HTTPS). Si no, se deduce de `X-Forwarded-Proto`.
    pub force_secure: bool,
}

impl AppState {
    pub fn new(db: Db) -> AppState {
        AppState { db, limiter: Arc::new(Limiter::new()), force_secure: false }
    }
}

/// Lo que el middleware averigua de cada petición y entrega a los manejadores.
pub struct ReqInfo {
    pub user: Option<User>,
    pub lang: Lang,
    pub path: String,
    pub query: String,
    pub csrf: String,
    pub ip: String,
    pub ua: String,
    pub https: bool,
    pub session_token: Option<String>,
}

impl ReqInfo {
    pub fn ctx(&self) -> Ctx {
        Ctx { lang: self.lang, user: self.user.clone(), path: self.path.clone(), query: self.query.clone(), csrf: self.csrf.clone() }
    }

    pub fn csrf_ok(&self, token: &str) -> bool {
        !self.csrf.is_empty() && util::ct_eq(&self.csrf, token)
    }

    pub fn secure_cookie(&self, st: &AppState) -> bool {
        self.https || st.force_secure
    }
}

/// Registra un evento de la persona que hace la petición.
pub fn log_event(st: &AppState, info: &ReqInfo, event: &str, method: &str, status: u16, detail: &str) {
    log_event_as(st, info.user.as_ref(), info, event, method, status, detail);
}

pub fn log_event_as(st: &AppState, user: Option<&User>, info: &ReqInfo, event: &str, method: &str, status: u16, detail: &str) {
    let label = user.map(|u| u.label()).unwrap_or_default();
    st.db.log(&NewActivity {
        user_id: user.map(|u| u.id),
        user_label: &label,
        role: user.map(|u| u.role),
        event,
        method,
        path: &info.path,
        query: "",
        status,
        ip: &info.ip,
        ua: &info.ua,
        detail,
    });
}

fn header_str<'a>(h: &'a HeaderMap, name: &str) -> &'a str {
    h.get(name).and_then(|v| v.to_str().ok()).unwrap_or("")
}

/// IP del cliente. Detrás de nginx (par local) se confía en `X-Real-IP`; en cualquier otro caso, en la conexión.
fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>) -> String {
    let behind_proxy = peer.is_none_or(|p| p.ip().is_loopback());
    if behind_proxy {
        let real = header_str(headers, "x-real-ip").trim();
        if real.parse::<IpAddr>().is_ok() {
            return real.to_string();
        }
        if let Some(first) = header_str(headers, "x-forwarded-for").split(',').next().map(str::trim) {
            if first.parse::<IpAddr>().is_ok() {
                return first.to_string();
            }
        }
    }
    peer.map(|p| p.ip().to_string()).unwrap_or_default()
}

fn is_public(path: &str) -> bool {
    path == "/login" || path == "/healthz" || path == "/favicon.ico" || path.starts_with("/static/")
}

fn is_fragment(path: &str) -> bool {
    path.ends_with("/benchmarks") || path.ends_with("/bokslut")
}

/// Con la contraseña temporal sin cambiar solo se puede ver el perfil y cerrar sesión.
fn allowed_while_must_change(path: &str) -> bool {
    matches!(path, "/profile" | "/profile/password" | "/logout") || is_public(path)
}

/// Evento de actividad de una página vista, o `None` si no se registra (estáticos, fragmentos, login).
fn page_event(method: &Method, path: &str, query: &str) -> Option<(&'static str, String)> {
    if method != Method::GET || is_public(path) || is_fragment(path) || path == "/activity.csv" || path == "/" {
        return None;
    }
    if path == "/sok" {
        return Some(match query_param(query, "q").filter(|q| !q.trim().is_empty()) {
            Some(q) => ("search", q.trim().chars().take(120).collect()),
            None => ("view", String::new()),
        });
    }
    if let Some(org) = path.strip_prefix("/foretag/").filter(|o| !o.contains('/')) {
        return Some(("company_view", bolagsverket::normalize_org_number(org).unwrap_or_else(|| org.to_string())));
    }
    Some(("view", String::new()))
}

const CSP: &str = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; \
font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'";

fn decorate(resp: &mut Response, lang_to_set: Option<Lang>, secure: bool) {
    let h = resp.headers_mut();
    if let Some(l) = lang_to_set {
        if let Ok(v) = HeaderValue::from_str(&auth::set_cookie(LANG_COOKIE, l.code(), 365 * 86_400, false, secure)) {
            h.append(header::SET_COOKIE, v);
        }
    }
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("same-origin"));
    h.insert("content-security-policy", HeaderValue::from_static(CSP));
    h.insert("x-robots-tag", HeaderValue::from_static("noindex, nofollow"));
    let is_html = h.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|t| t.starts_with("text/html"));
    if is_html {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    }
}

pub async fn session_mw(State(st): State<AppState>, mut req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = req.uri().query().unwrap_or("").to_string();
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    let (cookie_hdr, ua, ip, https, accept) = {
        let h = req.headers();
        (
            header_str(h, "cookie").to_string(),
            header_str(h, "user-agent").to_string(),
            client_ip(h, peer),
            header_str(h, "x-forwarded-proto") == "https",
            header_str(h, "accept-language").to_string(),
        )
    };

    let token = auth::cookie_value(&cookie_hdr, SESSION_COOKIE).filter(|t| t.len() == 64);
    let session = token.as_deref().and_then(|t| st.db.session_user(t));
    let (user, csrf) = match session {
        Some((u, c)) => (Some(u), c),
        None => (None, String::new()),
    };

    // Idioma: ?lang= > cookie > preferencia del perfil > Accept-Language > español.
    let lang_param = query_param(&query, "lang").and_then(|v| Lang::from_code(&v));
    let lang = lang_param
        .or_else(|| auth::cookie_value(&cookie_hdr, LANG_COOKIE).and_then(|v| Lang::from_code(&v)))
        .or_else(|| user.as_ref().map(|u| u.lang))
        .or_else(|| i18n::from_accept_language(&accept))
        .unwrap_or(Lang::DEFAULT);
    let lang_change = match (lang_param, user.as_ref()) {
        (Some(l), Some(u)) if u.lang != l => {
            st.db.set_lang(u.id, l);
            Some(format!("{}→{}", u.lang.code(), l.code()))
        }
        _ => None,
    };

    let info = Arc::new(ReqInfo { user, lang, path: path.clone(), query: query.clone(), csrf, ip, ua, https, session_token: token });
    let secure = info.secure_cookie(&st);
    if let Some(detail) = lang_change {
        log_event(&st, &info, "lang_change", method.as_str(), 200, &detail);
    }

    // Acceso obligatorio: todo salvo login, estáticos y salud.
    if info.user.is_none() && !is_public(&path) {
        let mut resp = if method == Method::GET && !is_fragment(&path) {
            let target = if query.is_empty() { path.clone() } else { format!("{path}?{query}") };
            Redirect::to(&format!("/login?next={}", urlencode(&target))).into_response()
        } else {
            StatusCode::UNAUTHORIZED.into_response()
        };
        decorate(&mut resp, lang_param, secure);
        return resp;
    }
    // Contraseña temporal: primero hay que cambiarla.
    if info.user.as_ref().is_some_and(|u| u.must_change) && !allowed_while_must_change(&path) {
        let mut resp = if is_fragment(&path) { StatusCode::FORBIDDEN.into_response() } else { Redirect::to("/profile?force=1").into_response() };
        decorate(&mut resp, lang_param, secure);
        return resp;
    }

    req.extensions_mut().insert(info.clone());
    let mut resp = next.run(req).await;
    let status = resp.status().as_u16();

    // Seguimiento: páginas vistas, búsquedas, empresas consultadas y accesos denegados.
    if let Some(user) = info.user.as_ref() {
        if status == 403 {
            log_event(&st, &info, "access_denied", method.as_str(), status, "");
        } else if status < 400 {
            if let Some((event, detail)) = page_event(&method, &path, &query) {
                st.db.log(&NewActivity {
                    user_id: Some(user.id),
                    user_label: &user.label(),
                    role: Some(user.role),
                    event,
                    method: method.as_str(),
                    path: &path,
                    // Solo se conservan parámetros que identifican la consulta; nunca contraseñas ni tokens.
                    query: &safe_query(&query),
                    status,
                    ip: &info.ip,
                    ua: &info.ua,
                    detail: &detail,
                });
            }
        }
    }
    decorate(&mut resp, lang_param, secure);
    resp
}

/// Parámetros de la query que se guardan en el registro (lista cerrada).
fn safe_query(query: &str) -> String {
    query
        .split('&')
        .filter(|kv| matches!(kv.split('=').next(), Some("q" | "tab" | "sort" | "dir" | "user" | "event" | "page" | "from" | "to" | "role")))
        .collect::<Vec<_>>()
        .join("&")
}

/// Sólo rutas internas como destino tras el login (evita redirecciones abiertas).
pub fn safe_next(next: &str) -> String {
    if next.starts_with('/') && !next.starts_with("//") && !next.contains('\\') && !next.starts_with("/login") {
        next.to_string()
    } else {
        "/sok".to_string()
    }
}

pub fn role_from_form(code: &str) -> Option<Role> {
    Role::from_code(code.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_redirects_stay_internal() {
        assert_eq!(safe_next("/foretag/559012-3456?tab=fin"), "/foretag/559012-3456?tab=fin");
        assert_eq!(safe_next("https://malo.example"), "/sok");
        assert_eq!(safe_next("//malo.example"), "/sok");
        assert_eq!(safe_next("/\\malo.example"), "/sok");
        assert_eq!(safe_next("/login"), "/sok");
        assert_eq!(safe_next(""), "/sok");
    }

    #[test]
    fn page_events() {
        let get = Method::GET;
        assert_eq!(page_event(&get, "/sok", "q=g%C3%B6teborg"), Some(("search", "göteborg".to_string())));
        assert_eq!(page_event(&get, "/sok", ""), Some(("view", String::new())));
        assert_eq!(page_event(&get, "/foretag/556703-7485", ""), Some(("company_view", "5567037485".to_string())));
        assert_eq!(page_event(&get, "/foretag/556703-7485/bokslut", ""), None, "los fragmentos no se registran");
        assert_eq!(page_event(&get, "/static/styles.css", ""), None);
        assert_eq!(page_event(&Method::POST, "/sok", ""), None);
        assert_eq!(page_event(&get, "/likviditet", ""), Some(("view", String::new())));
    }

    #[test]
    fn logged_query_keeps_only_whitelisted_params() {
        assert_eq!(safe_query("q=a&password=secreto&tab=fin&token=x"), "q=a&tab=fin");
    }

    #[test]
    fn client_ip_trusts_proxy_headers_only_from_loopback() {
        let mut h = HeaderMap::new();
        h.insert("x-real-ip", HeaderValue::from_static("203.0.113.9"));
        let local: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let remote: SocketAddr = "198.51.100.7:5000".parse().unwrap();
        assert_eq!(client_ip(&h, Some(local)), "203.0.113.9");
        assert_eq!(client_ip(&h, Some(remote)), "198.51.100.7", "un cliente directo no puede falsear su IP");
        assert_eq!(client_ip(&h, None), "203.0.113.9");
        let mut bad = HeaderMap::new();
        bad.insert("x-real-ip", HeaderValue::from_static("no-es-una-ip"));
        assert_eq!(client_ip(&bad, Some(local)), "127.0.0.1");
    }
}
