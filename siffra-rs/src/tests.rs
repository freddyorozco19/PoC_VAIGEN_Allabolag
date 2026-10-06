//! Pruebas de la aplicación completa (rutas, sesiones, permisos, actividad e idiomas) contra una base de
//! datos SQLite en memoria. No tocan la red: SCB desactivado y Bolagsverket simulado en local.

use std::collections::BTreeSet;

use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::app::AppState;
use crate::catalog::ENTRIES;
use crate::db::{ActivityFilter, Db, NewUser, Role, User};
use crate::i18n::{self, Lang};
use crate::util::urlencode;
use crate::views::Ctx;
use crate::{annual_report, auth, bolagsverket, model, views};

/// Los tests que dependen de las variables BOLAGSVERKET_* (globales del proceso) no pueden correr a la vez.
static BOLAGSVERKET_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_bolagsverket_env() -> std::sync::MutexGuard<'static, ()> {
    BOLAGSVERKET_ENV.lock().unwrap_or_else(|e| e.into_inner())
}

// ───────────────────────── Utilidades ─────────────────────────

struct Resp {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Resp {
    fn location(&self) -> Option<String> {
        self.headers.get(header::LOCATION).map(|v| v.to_str().unwrap().to_string())
    }
    fn set_cookies(&self) -> Vec<String> {
        self.headers.get_all(header::SET_COOKIE).iter().map(|v| v.to_str().unwrap().to_string()).collect()
    }
    /// Línea completa de la cookie `name` recibida, p. ej. `siffra_sid=abc; Path=/; HttpOnly`.
    fn cookie_line(&self, name: &str) -> Option<String> {
        self.set_cookies().into_iter().find(|c| c.starts_with(&format!("{name}=")))
    }
    fn cookie_value(&self, name: &str) -> Option<String> {
        self.cookie_line(name).map(|l| l[name.len() + 1..].split(';').next().unwrap().to_string())
    }
}

async fn send(st: &AppState, method: &str, path: &str, cookie: Option<&str>, body: Option<String>, headers: &[(&str, &str)]) -> Resp {
    // Las pruebas de rutas no deben depender de la red: SCB desactivado → medianas de EJEMPLO.
    std::env::set_var("SCB_STATS_DISABLED", "1");
    let mut b = Request::builder().method(method).uri(path);
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, c);
    }
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let req = match body {
        Some(f) => b.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded").body(Body::from(f)).unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let res = crate::router(st.clone()).oneshot(req).await.unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    Resp { status, headers, body: String::from_utf8(bytes.to_vec()).unwrap() }
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v))).collect::<Vec<_>>().join("&")
}

struct Sess {
    cookie: String,
    csrf: String,
}

const PW: &str = "Clave-de-prueba-1";

fn seed(st: &AppState, role: Role, email: Option<&str>, username: Option<&str>, name: &str, must_change: bool) -> User {
    let id = st
        .db
        .create_user(&NewUser {
            email: email.map(String::from),
            username: username.map(String::from),
            name: name.into(),
            role,
            active: true,
            lang: Lang::Es,
            pass_hash: auth::hash_password(PW),
            must_change,
            created_by: None,
        })
        .unwrap();
    st.db.user_by_id(id).unwrap()
}

fn session_for(st: &AppState, u: &User) -> Sess {
    let (token, csrf) = st.db.create_session(u.id, auth::SESSION_TTL_SECS, "10.0.0.1", "test-agent");
    Sess { cookie: format!("siffra_sid={token}"), csrf }
}

/// Tres cuentas con los roles pedidos: superadmin (usuario `architechia`), admin y user.
struct Fx {
    st: AppState,
    sa: User,
    admin: User,
    user: User,
    sa_s: Sess,
    admin_s: Sess,
    user_s: Sess,
}

fn fx() -> Fx {
    let mut st = AppState::new(Db::memory());
    st.demo = true; // casi todas las pruebas usan las empresas de EJEMPLO
    let sa = seed(&st, Role::Superadmin, None, Some("architechia"), "Architechia", false);
    let admin = seed(&st, Role::Admin, Some("freddy.orozco@architechia.co"), None, "Freddy Orozco", false);
    let user = seed(&st, Role::User, Some("testing@architechia.co"), None, "Testing", false);
    let (sa_s, admin_s, user_s) = (session_for(&st, &sa), session_for(&st, &admin), session_for(&st, &user));
    Fx { st, sa, admin, user, sa_s, admin_s, user_s }
}

impl Fx {
    async fn get(&self, s: &Sess, path: &str) -> Resp {
        send(&self.st, "GET", path, Some(&s.cookie), None, &[]).await
    }
    async fn get_anon(&self, path: &str) -> Resp {
        send(&self.st, "GET", path, None, None, &[]).await
    }
    /// POST con el token CSRF de la sesión.
    async fn post(&self, s: &Sess, path: &str, pairs: &[(&str, &str)]) -> Resp {
        let mut all = vec![("csrf", s.csrf.as_str())];
        all.extend_from_slice(pairs);
        send(&self.st, "POST", path, Some(&s.cookie), Some(form(&all)), &[]).await
    }
    async fn post_no_csrf(&self, s: &Sess, path: &str, pairs: &[(&str, &str)]) -> Resp {
        send(&self.st, "POST", path, Some(&s.cookie), Some(form(pairs)), &[]).await
    }
    /// Inicio de sesión completo: obtiene el token previo y envía el formulario.
    async fn login(&self, ident: &str, password: &str, next: &str) -> Resp {
        let page = self.get_anon("/login").await;
        let pre = page.cookie_value("siffra_pre").expect("cookie previa");
        send(
            &self.st,
            "POST",
            "/login",
            Some(&format!("siffra_pre={pre}")),
            Some(form(&[("csrf", &pre), ("identifier", ident), ("password", password), ("next", next)])),
            &[],
        )
        .await
    }
    fn events(&self) -> Vec<String> {
        self.st.db.activity(&ActivityFilter::default(), 500, 0).0.into_iter().map(|r| r.event).collect()
    }
}

fn session_from(resp: &Resp) -> Sess {
    Sess { cookie: format!("siffra_sid={}", resp.cookie_value("siffra_sid").expect("cookie de sesión")), csrf: String::new() }
}

fn temp_password(body: &str) -> String {
    let marker = "class=\"temp-password mono\">";
    let start = body.find(marker).expect("contraseña temporal en la página") + marker.len();
    body[start..start + body[start..].find("</span>").unwrap()].to_string()
}

// ───────────────────────── Acceso obligatorio ─────────────────────────

#[tokio::test]
async fn everything_requires_login_except_login_static_and_health() {
    let f = fx();
    let r = f.get_anon("/sok?q=uppsala").await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(r.location().as_deref(), Some("/login?next=%2Fsok%3Fq%3Duppsala"));
    for path in ["/", "/foretag/559012-3456", "/bevakning", "/likviditet", "/sie", "/fakturor", "/users", "/activity", "/profile", "/activity.csv"] {
        let r = f.get_anon(path).await;
        assert_eq!(r.status, StatusCode::SEE_OTHER, "{path}");
        assert!(r.location().unwrap().starts_with("/login?next="), "{path}");
    }
    // Los fragmentos y los POST sin sesión no redirigen: 401.
    assert_eq!(f.get_anon("/foretag/559012-3456/benchmarks").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(f.get_anon("/foretag/559012-3456/bokslut").await.status, StatusCode::UNAUTHORIZED);
    let r = send(&f.st, "POST", "/users", None, Some(form(&[("name", "x")])), &[]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // Públicas.
    assert_eq!(f.get_anon("/login").await.status, StatusCode::OK);
    assert_eq!(f.get_anon("/healthz").await.body, "ok");
    assert_eq!(f.get_anon("/static/styles.css").await.status, StatusCode::OK);
    // Una sesión inventada no vale.
    let fake = Sess { cookie: format!("siffra_sid={}", "a".repeat(64)), csrf: String::new() };
    assert_eq!(f.get(&fake, "/sok").await.status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn root_redirects_to_sok_when_signed_in() {
    let f = fx();
    let r = f.get(&f.user_s, "/").await;
    assert_eq!(r.status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(r.location().as_deref(), Some("/sok"));
}

#[tokio::test]
async fn login_with_username_or_email_sets_a_hardened_session_cookie() {
    let f = fx();
    let page = f.get_anon("/login").await;
    assert!(page.body.contains(r#"name="identifier""#) && page.body.contains(r#"type="password""#));
    let pre = page.cookie_value("siffra_pre").unwrap();
    assert!(page.body.contains(&format!(r#"name="csrf" value="{pre}""#)));
    assert!(page.cookie_line("siffra_pre").unwrap().contains("HttpOnly"));

    // Superadmin: usuario "architechia" (sin distinguir mayúsculas).
    let r = f.login("Architechia", PW, "/likviditet").await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(r.location().as_deref(), Some("/likviditet"));
    let sid = r.cookie_line("siffra_sid").unwrap();
    assert!(sid.contains("HttpOnly") && sid.contains("SameSite=Lax") && sid.contains("Max-Age=43200"), "{sid}");
    assert!(!sid.contains("Secure"), "sin HTTPS no se marca Secure");
    assert_eq!(f.get(&session_from(&r), "/sok").await.status, StatusCode::OK);

    // Admin y user entran con el correo.
    for email in ["freddy.orozco@architechia.co", "testing@architechia.co"] {
        let r = f.login(email, PW, "").await;
        assert_eq!(r.status, StatusCode::SEE_OTHER, "{email}");
        assert_eq!(r.location().as_deref(), Some("/sok"));
    }
    // El token no se guarda en claro.
    let token = r.cookie_value("siffra_sid").unwrap();
    assert_eq!(token.len(), 64);
    assert_ne!(token, util_hash(&token));
}

fn util_hash(s: &str) -> String {
    crate::util::sha256_hex(s)
}

#[tokio::test]
async fn session_cookie_is_secure_behind_https_proxy() {
    let f = fx();
    let page = send(&f.st, "GET", "/login", None, None, &[("x-forwarded-proto", "https")]).await;
    let pre = page.cookie_value("siffra_pre").unwrap();
    assert!(page.cookie_line("siffra_pre").unwrap().contains("Secure"));
    let r = send(
        &f.st,
        "POST",
        "/login",
        Some(&format!("siffra_pre={pre}")),
        Some(form(&[("csrf", &pre), ("identifier", "architechia"), ("password", PW)])),
        &[("x-forwarded-proto", "https"), ("x-real-ip", "203.0.113.7")],
    )
    .await;
    assert!(r.cookie_line("siffra_sid").unwrap().contains("Secure"));
    let (rows, _) = f.st.db.activity(&ActivityFilter::default(), 5, 0);
    assert_eq!(rows[0].event, "login_ok");
    assert_eq!(rows[0].ip, "203.0.113.7", "IP real vía X-Real-IP del proxy local");
}

#[tokio::test]
async fn wrong_credentials_are_rejected_and_recorded_without_the_password() {
    let f = fx();
    let r = f.login("architechia", "Secreto-equivocado-9", "").await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert!(r.body.contains("Usuario o contraseña incorrectos"));
    assert!(r.cookie_line("siffra_sid").is_none());
    assert!(r.body.contains(r#"value="architechia""#), "se conserva lo escrito (no la contraseña)");
    assert!(!r.body.contains("Secreto-equivocado-9"));
    // Usuario inexistente: mismo mensaje (no revela qué cuentas existen).
    let r2 = f.login("nadie@x.co", PW, "").await;
    assert_eq!(r2.status, StatusCode::UNAUTHORIZED);
    assert!(r2.body.contains("Usuario o contraseña incorrectos"));
    // Cuenta desactivada: tampoco entra.
    f.st.db.update_user(f.user.id, "Testing", Some("testing@architechia.co"), None, Role::User, false, Lang::Es).unwrap();
    assert_eq!(f.login("testing@architechia.co", PW, "").await.status, StatusCode::UNAUTHORIZED);

    let (rows, _) = f.st.db.activity(&ActivityFilter { event: Some("login_fail".into()), ..Default::default() }, 10, 0);
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().any(|r| r.detail == "nadie@x.co"));
    let all = format!("{rows:?}");
    assert!(!all.contains("Secreto-equivocado-9") && !all.contains(PW), "nunca se guarda una contraseña");
}

#[tokio::test]
async fn login_needs_the_pre_login_csrf_token() {
    let f = fx();
    let page = f.get_anon("/login").await;
    let pre = page.cookie_value("siffra_pre").unwrap();
    // Sin cookie previa.
    let r = send(&f.st, "POST", "/login", None, Some(form(&[("csrf", &pre), ("identifier", "architechia"), ("password", PW)])), &[]).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    // Con un token que no coincide.
    let r = send(
        &f.st,
        "POST",
        "/login",
        Some(&format!("siffra_pre={pre}")),
        Some(form(&[("csrf", "otro"), ("identifier", "architechia"), ("password", PW)])),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(r.cookie_line("siffra_sid").is_none());
}

#[tokio::test]
async fn login_is_rate_limited_after_five_failures() {
    let f = fx();
    for _ in 0..5 {
        assert_eq!(f.login("architechia", "mala-clave-1", "").await.status, StatusCode::UNAUTHORIZED);
    }
    // Aun con la contraseña correcta: bloqueado.
    let r = f.login("architechia", PW, "").await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(r.body.contains("Demasiados intentos"));
    assert!(r.cookie_line("siffra_sid").is_none());
    assert!(f.events().contains(&"login_blocked".to_string()));
}

#[tokio::test]
async fn login_redirect_target_must_be_internal() {
    let f = fx();
    for evil in ["https://malo.example/", "//malo.example", "/\\malo.example"] {
        let r = f.login("architechia", PW, evil).await;
        assert_eq!(r.location().as_deref(), Some("/sok"), "{evil}");
    }
    // Ya con sesión, /login lleva a la app.
    assert_eq!(f.get(&f.sa_s, "/login").await.status, StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn logout_requires_csrf_and_kills_the_session() {
    let f = fx();
    let r = f.post_no_csrf(&f.user_s, "/logout", &[]).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(f.get(&f.user_s, "/sok").await.status, StatusCode::OK, "la sesión sigue viva");
    let r = f.post(&f.user_s, "/logout", &[]).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(r.location().as_deref(), Some("/login"));
    assert!(r.cookie_line("siffra_sid").unwrap().contains("Max-Age=0"));
    assert_eq!(f.get(&f.user_s, "/sok").await.status, StatusCode::SEE_OTHER, "el token ya no vale");
    assert!(f.events().contains(&"logout".to_string()));
}

#[tokio::test]
async fn security_headers_are_present() {
    let f = fx();
    let r = f.get(&f.sa_s, "/sok").await;
    let h = |n: &str| r.headers.get(n).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
    assert!(h("content-security-policy").contains("frame-ancestors 'none'"));
    assert_eq!(h("x-frame-options"), "DENY");
    assert_eq!(h("x-content-type-options"), "nosniff");
    assert_eq!(h("cache-control"), "private, no-store");
    assert!(h("x-robots-tag").contains("noindex"));
    assert!(r.body.contains(r#"<meta name="referrer" content="same-origin">"#));
}

// ───────────────────────── Permisos por rol ─────────────────────────

#[tokio::test]
async fn role_based_access_to_admin_screens() {
    let f = fx();
    for path in ["/users", "/users/new", "/activity", "/activity.csv"] {
        assert_eq!(f.get(&f.user_s, path).await.status, StatusCode::FORBIDDEN, "user {path}");
    }
    assert_eq!(f.get(&f.admin_s, "/users").await.status, StatusCode::OK);
    assert_eq!(f.get(&f.admin_s, "/users/new").await.status, StatusCode::OK);
    assert_eq!(f.get(&f.admin_s, "/activity").await.status, StatusCode::FORBIDDEN, "el admin no ve la actividad");
    assert_eq!(f.get(&f.admin_s, "/activity.csv").await.status, StatusCode::FORBIDDEN);
    assert_eq!(f.get(&f.sa_s, "/users").await.status, StatusCode::OK);
    assert_eq!(f.get(&f.sa_s, "/activity").await.status, StatusCode::OK);
    assert_eq!(f.get(&f.sa_s, "/activity.csv").await.status, StatusCode::OK);
    // La navegación solo ofrece lo permitido.
    let nav = |body: &str| (body.contains(r#"href="/users""#), body.contains(r#"href="/activity""#));
    assert_eq!(nav(&f.get(&f.user_s, "/sok").await.body), (false, false));
    assert_eq!(nav(&f.get(&f.admin_s, "/sok").await.body), (true, false));
    assert_eq!(nav(&f.get(&f.sa_s, "/sok").await.body), (true, true));
    // Los 403 se registran.
    assert!(f.events().contains(&"access_denied".to_string()));
}

#[tokio::test]
async fn admin_manages_only_users_with_role_user() {
    let f = fx();
    // Crea un usuario.
    let r = f
        .post(&f.admin_s, "/users", &[("name", "Marta Gil"), ("email", "marta@x.co"), ("role", "user"), ("lang", "es"), ("password", "Una-frase-larga-9")])
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);
    let marta = f.st.db.user_by_login("marta@x.co").unwrap().0;
    assert_eq!((marta.role, marta.created_by), (Role::User, Some(f.admin.id)));
    // No puede crear admins ni superadmins.
    for role in ["admin", "superadmin"] {
        let r = f.post(&f.admin_s, "/users", &[("name", "X"), ("email", &format!("{role}@x.co")), ("role", role), ("lang", "es")]).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{role}");
        assert!(f.st.db.user_by_login(&format!("{role}@x.co")).is_none());
    }
    // No puede abrir ni modificar ni borrar ni restablecer a un superadmin o a otro admin.
    let other_admin = seed(&f.st, Role::Admin, Some("otro@x.co"), None, "Otro Admin", false);
    for target in [f.sa.id, other_admin.id] {
        assert_eq!(f.get(&f.admin_s, &format!("/users/{target}/edit")).await.status, StatusCode::FORBIDDEN);
        assert_eq!(f.get(&f.admin_s, &format!("/users/{target}/delete")).await.status, StatusCode::FORBIDDEN);
        assert_eq!(f.post(&f.admin_s, &format!("/users/{target}"), &[("name", "Hack"), ("email", "h@x.co"), ("role", "user"), ("lang", "es"), ("active", "1")]).await.status, StatusCode::FORBIDDEN);
        assert_eq!(f.post(&f.admin_s, &format!("/users/{target}/delete"), &[]).await.status, StatusCode::FORBIDDEN);
        assert_eq!(f.post(&f.admin_s, &format!("/users/{target}/reset-password"), &[]).await.status, StatusCode::FORBIDDEN);
    }
    assert_eq!(f.st.db.user_by_id(f.sa.id).unwrap().role, Role::Superadmin);
    // No puede ascender a un usuario.
    let r = f.post(&f.admin_s, &format!("/users/{}", marta.id), &[("name", "Marta"), ("email", "marta@x.co"), ("role", "admin"), ("lang", "es"), ("active", "1")]).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(f.st.db.user_by_id(marta.id).unwrap().role, Role::User);
    // Sí puede borrarla.
    assert_eq!(f.post(&f.admin_s, &format!("/users/{}/delete", marta.id), &[]).await.status, StatusCode::SEE_OTHER);
    assert!(f.st.db.user_by_id(marta.id).is_none());
    // En su listado no aparecen acciones sobre superadmins.
    let list = f.get(&f.admin_s, "/users").await.body;
    assert!(!list.contains(&format!("/users/{}/edit", f.sa.id)));
    assert!(list.contains(&format!("/users/{}/edit", f.user.id)));
}

#[tokio::test]
async fn superadmin_crud_cycle() {
    let f = fx();
    // Alta con contraseña propia.
    let r = f
        .post(&f.sa_s, "/users", &[("name", "Marta Gil"), ("email", "marta@x.co"), ("role", "user"), ("lang", "sv"), ("password", "Una-frase-larga-9")])
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(r.location().as_deref(), Some("/users?ok=created"));
    let marta = f.st.db.user_by_login("marta@x.co").unwrap().0;
    assert_eq!((marta.lang, marta.role, marta.must_change, marta.active), (Lang::Sv, Role::User, false, true));
    let list = f.get(&f.sa_s, "/users?ok=created").await.body;
    assert!(list.contains("Marta Gil") && list.contains("Usuario creado."));
    // Duplicados.
    let r = f.post(&f.sa_s, "/users", &[("name", "Otra"), ("email", "MARTA@x.co"), ("role", "user"), ("lang", "es")]).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(r.body.contains("Ya existe un usuario con ese correo"));
    assert!(r.body.contains(r#"value="Otra""#), "el formulario conserva lo escrito");
    // Validaciones.
    for (pairs, text) in [
        (vec![("name", ""), ("email", "a@x.co"), ("role", "user")], "El nombre es obligatorio"),
        (vec![("name", "A"), ("email", "no-es-correo"), ("role", "user")], "no es válido"),
        (vec![("name", "A"), ("email", ""), ("username", ""), ("role", "user")], "Indica un correo"),
        (vec![("name", "A"), ("username", "a b"), ("role", "user")], "Nombre de usuario no válido"),
        (vec![("name", "A"), ("email", "a@x.co"), ("role", "user"), ("password", "corta")], "demasiado corta"),
        (vec![("name", "A"), ("email", "a@x.co"), ("role", "dios")], "No puedes asignar ese rol"),
    ] {
        let r = f.post(&f.sa_s, "/users", &pairs).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{text}");
        assert!(r.body.contains(text), "{text}");
    }
    // Edición: cambia nombre, rol e idioma, y cierra sus sesiones.
    let marta_s = session_for(&f.st, &marta);
    let r = f.post(&f.sa_s, &format!("/users/{}", marta.id), &[("name", "Marta G."), ("email", "marta@x.co"), ("role", "admin"), ("lang", "en"), ("active", "1")]).await;
    assert_eq!(r.location().as_deref(), Some("/users?ok=updated"));
    let m = f.st.db.user_by_id(marta.id).unwrap();
    assert_eq!((m.name.as_str(), m.role, m.lang), ("Marta G.", Role::Admin, Lang::En));
    assert_eq!(f.get(&marta_s, "/sok").await.status, StatusCode::SEE_OTHER, "un cambio de rol cierra sus sesiones");
    // Desactivar impide entrar.
    f.post(&f.sa_s, &format!("/users/{}", marta.id), &[("name", "Marta G."), ("email", "marta@x.co"), ("role", "admin"), ("lang", "en")]).await;
    assert!(!f.st.db.user_by_id(marta.id).unwrap().active);
    assert_eq!(f.login("marta@x.co", "Una-frase-larga-9", "").await.status, StatusCode::UNAUTHORIZED);
    // Restablecer contraseña: se muestra una vez y obliga a cambiarla.
    let r = f.post(&f.sa_s, &format!("/users/{}/reset-password", marta.id), &[]).await;
    assert_eq!(r.status, StatusCode::OK);
    let temp = temp_password(&r.body);
    assert_eq!(temp.len(), 14);
    f.post(&f.sa_s, &format!("/users/{}", marta.id), &[("name", "Marta G."), ("email", "marta@x.co"), ("role", "admin"), ("lang", "en"), ("active", "1")]).await;
    let r = f.login("marta@x.co", &temp, "").await;
    assert_eq!(r.location().as_deref(), Some("/profile?force=1"));
    // Borrado con confirmación.
    assert_eq!(f.get(&f.sa_s, &format!("/users/{}/delete", marta.id)).await.status, StatusCode::OK);
    let r = f.post(&f.sa_s, &format!("/users/{}/delete", marta.id), &[]).await;
    assert_eq!(r.location().as_deref(), Some("/users?ok=deleted"));
    assert!(f.st.db.user_by_id(marta.id).is_none());
    let events = f.events();
    for e in ["user_create", "user_update", "password_reset", "user_delete"] {
        assert!(events.contains(&e.to_string()), "{e} registrado");
    }
}

#[tokio::test]
async fn created_without_password_gets_a_one_time_temporary_one() {
    let f = fx();
    let r = f.post(&f.sa_s, "/users", &[("name", "Nuevo"), ("email", "nuevo@x.co"), ("role", "user"), ("lang", "es")]).await;
    assert_eq!(r.status, StatusCode::OK);
    let temp = temp_password(&r.body);
    let u = f.st.db.user_by_login("nuevo@x.co").unwrap().0;
    assert!(u.must_change);
    assert_eq!(f.login("nuevo@x.co", &temp, "").await.location().as_deref(), Some("/profile?force=1"));
    assert!(!f.get(&f.sa_s, "/users").await.body.contains(&temp), "no vuelve a mostrarse");
}

#[tokio::test]
async fn nobody_can_lock_the_platform_out() {
    let f = fx();
    // El superadmin no se borra ni se degrada ni se desactiva a sí mismo.
    assert_eq!(f.get(&f.sa_s, &format!("/users/{}/delete", f.sa.id)).await.status, StatusCode::FORBIDDEN);
    assert_eq!(f.post(&f.sa_s, &format!("/users/{}/delete", f.sa.id), &[]).await.status, StatusCode::FORBIDDEN);
    let r = f.post(&f.sa_s, &format!("/users/{}", f.sa.id), &[("name", "A"), ("username", "architechia"), ("role", "admin"), ("lang", "es"), ("active", "1")]).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = f.post(&f.sa_s, &format!("/users/{}", f.sa.id), &[("name", "A"), ("username", "architechia"), ("role", "superadmin"), ("lang", "es")]).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "tampoco desactivarse");
    assert_eq!(f.st.db.count_active_superadmins(), 1);
    // Con dos superadmins, uno puede quitar al otro, pero no al último.
    let second = seed(&f.st, Role::Superadmin, Some("dos@x.co"), None, "Dos", false);
    let second_s = session_for(&f.st, &second);
    assert_eq!(f.post(&second_s, &format!("/users/{}/delete", f.sa.id), &[]).await.status, StatusCode::SEE_OTHER);
    assert_eq!(f.st.db.count_active_superadmins(), 1);
    assert_eq!(f.post(&second_s, &format!("/users/{}/delete", second.id), &[]).await.status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn state_changing_forms_need_the_csrf_token() {
    let f = fx();
    let pairs = [("name", "Intruso"), ("email", "intruso@x.co"), ("role", "user"), ("lang", "es")];
    assert_eq!(f.post_no_csrf(&f.sa_s, "/users", &pairs).await.status, StatusCode::FORBIDDEN);
    assert!(f.st.db.user_by_login("intruso@x.co").is_none());
    let bad = Sess { cookie: f.sa_s.cookie.clone(), csrf: "token-falso".into() };
    assert_eq!(f.post(&bad, "/users", &pairs).await.status, StatusCode::FORBIDDEN);
    let uid = f.user.id;
    assert_eq!(f.post_no_csrf(&f.sa_s, &format!("/users/{uid}/delete"), &[]).await.status, StatusCode::FORBIDDEN);
    assert!(f.st.db.user_by_id(uid).is_some());
    assert_eq!(f.post_no_csrf(&f.sa_s, &format!("/users/{uid}/reset-password"), &[]).await.status, StatusCode::FORBIDDEN);
    assert_eq!(f.post_no_csrf(&f.user_s, "/profile", &[("name", "X"), ("lang", "en")]).await.status, StatusCode::FORBIDDEN);
    assert_eq!(f.post_no_csrf(&f.user_s, "/profile/password", &[("current", PW), ("new1", "Nueva-clave-123"), ("new2", "Nueva-clave-123")]).await.status, StatusCode::FORBIDDEN);
    assert!(f.st.db.user_by_login("testing@architechia.co").is_some());
    assert!(auth::verify_password(&f.st.db.hash_of(uid).unwrap(), PW), "la contraseña no cambió");
}

// ───────────────────────── Perfil y contraseña ─────────────────────────

#[tokio::test]
async fn temporary_password_forces_a_change_before_anything_else() {
    let f = fx();
    let tmp = seed(&f.st, Role::User, Some("temp@x.co"), None, "Temporal", true);
    let s = session_for(&f.st, &tmp);
    let r = f.get(&s, "/sok").await;
    assert_eq!((r.status, r.location().as_deref()), (StatusCode::SEE_OTHER, Some("/profile?force=1")));
    assert_eq!(f.get(&s, "/foretag/559012-3456/benchmarks").await.status, StatusCode::FORBIDDEN);
    let page = f.get(&s, "/profile?force=1").await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("Tu contraseña es temporal"));

    let change = |cur: &'static str, a: &'static str, b: &'static str| {
        let (f, s) = (&f, &s);
        async move { f.post(s, "/profile/password", &[("current", cur), ("new1", a), ("new2", b)]).await }
    };
    let r = change("incorrecta", "Nueva-clave-123", "Nueva-clave-123").await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(r.body.contains("no es correcta"));
    assert!(change(PW, "Nueva-clave-123", "Otra-distinta-123").await.body.contains("no coinciden"));
    assert!(change(PW, "corta", "corta").await.body.contains("demasiado corta"));
    assert!(change(PW, "1234567890123", "1234567890123").await.body.contains("demasiado simple"));
    assert!(change(PW, PW, PW).await.body.contains("distinta de la actual"));
    // Correcto: sesión nueva, la vieja deja de valer, y ya puede navegar.
    let r = change(PW, "Nueva-clave-123", "Nueva-clave-123").await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    let fresh = session_from(&r);
    assert_eq!(f.get(&s, "/sok").await.status, StatusCode::SEE_OTHER, "sesión anterior cerrada");
    assert_eq!(f.get(&fresh, "/sok").await.status, StatusCode::OK);
    assert!(!f.st.db.user_by_id(tmp.id).unwrap().must_change);
    assert_eq!(f.login("temp@x.co", PW, "").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(f.login("temp@x.co", "Nueva-clave-123", "").await.status, StatusCode::SEE_OTHER);
    assert!(f.events().contains(&"password_change".to_string()));
}

#[tokio::test]
async fn profile_edit_changes_name_and_language() {
    let f = fx();
    let page = f.get(&f.user_s, "/profile").await;
    assert!(page.body.contains("Mis datos") && page.body.contains("testing@architechia.co"));
    let r = f.post(&f.user_s, "/profile", &[("name", "Tester Uno"), ("lang", "sv")]).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("Profilen har sparats"), "la respuesta ya sale en el idioma nuevo");
    let u = f.st.db.user_by_id(f.user.id).unwrap();
    assert_eq!((u.name.as_str(), u.lang), ("Tester Uno", Lang::Sv));
    assert_eq!(r.cookie_value("lang").as_deref(), Some("sv"));
    // Nombre vacío → error.
    let r = f.post(&f.user_s, "/profile", &[("name", "  "), ("lang", "es")]).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    // Una cuenta no puede cambiar su rol por esta vía.
    f.post(&f.user_s, "/profile", &[("name", "T"), ("lang", "es"), ("role", "superadmin")]).await;
    assert_eq!(f.st.db.user_by_id(f.user.id).unwrap().role, Role::User);
}

// ───────────────────────── Seguimiento de actividad ─────────────────────────

#[tokio::test]
async fn activity_tracks_what_each_user_does_and_only_superadmin_sees_it() {
    let f = fx();
    f.get(&f.user_s, "/sok").await;
    f.get(&f.user_s, "/sok?q=uppsala").await;
    f.get(&f.user_s, "/foretag/559012-3456").await;
    f.get(&f.user_s, "/foretag/559012-3456/benchmarks").await; // fragmento: no cuenta como página vista
    f.get(&f.user_s, "/likviditet?lang=en").await; // cambio de idioma
    f.get(&f.user_s, "/users").await; // acceso denegado
    f.get(&f.admin_s, "/sok?q=g%C3%B6teborg").await;

    let (rows, _) = f.st.db.activity(&ActivityFilter { user_id: Some(f.user.id), ..Default::default() }, 100, 0);
    let seq: Vec<(&str, &str)> = rows.iter().rev().map(|r| (r.event.as_str(), r.detail.as_str())).collect();
    assert_eq!(
        seq,
        [
            ("view", ""),
            ("search", "uppsala"),
            ("company_view", "5590123456"),
            ("lang_change", "es→en"),
            ("view", ""),
            ("access_denied", ""),
        ]
    );
    assert!(rows.iter().all(|r| r.ip == "" || !r.ip.is_empty()) && rows.iter().all(|r| r.ua == "" || r.ua == "test-agent"));
    assert!(rows.iter().all(|r| r.user_label == "testing@architechia.co" && r.role.as_deref() == Some("user")));

    let page = f.get(&f.sa_s, "/activity").await;
    assert_eq!(page.status, StatusCode::OK);
    for needle in ["testing@architechia.co", "freddy.orozco@architechia.co", "uppsala", "göteborg", "5590123456", "Empresa consultada", "Acceso denegado", "Cambio de idioma"] {
        assert!(page.body.contains(needle), "la actividad no muestra {needle:?}");
    }
    // Filtros.
    let by_user = f.get(&f.sa_s, &format!("/activity?user={}", f.admin.id)).await.body;
    assert!(by_user.contains("göteborg") && !by_user.contains("uppsala"));
    let by_event = f.get(&f.sa_s, "/activity?event=search").await.body;
    assert!(by_event.contains("uppsala") && !by_event.contains("Empresa consultada</span>"));
    let by_text = f.get(&f.sa_s, "/activity?q=5590123456").await.body;
    assert!(by_text.contains("5590123456"));
    // Resumen por usuario.
    let summary = f.st.db.user_summaries();
    let t = summary.iter().find(|s| s.user_id == f.user.id).unwrap();
    assert_eq!((t.views, t.searches, t.companies), (4, 1, 1));
}

#[tokio::test]
async fn login_logout_and_failures_appear_in_the_log_with_ip() {
    let f = fx();
    let r = send(&f.st, "GET", "/login", None, None, &[]).await;
    let pre = r.cookie_value("siffra_pre").unwrap();
    let headers = [("x-real-ip", "198.51.100.23"), ("user-agent", "Mozilla/5.0 Prueba")];
    let attempt = |pw: &'static str| {
        let (st, cookie, body) = (f.st.clone(), format!("siffra_pre={pre}"), form(&[("csrf", &pre), ("identifier", "testing@architechia.co"), ("password", pw)]));
        async move { send(&st, "POST", "/login", Some(&cookie), Some(body), &headers).await }
    };
    attempt("mal-mal-mal-1").await;
    attempt(PW).await;
    let (rows, _) = f.st.db.activity(&ActivityFilter::default(), 10, 0);
    let ok = rows.iter().find(|r| r.event == "login_ok").unwrap();
    assert_eq!((ok.ip.as_str(), ok.ua.as_str(), ok.user_label.as_str()), ("198.51.100.23", "Mozilla/5.0 Prueba", "testing@architechia.co"));
    let fail = rows.iter().find(|r| r.event == "login_fail").unwrap();
    assert_eq!((fail.ip.as_str(), fail.user_id, fail.status), ("198.51.100.23", None, 401));
    // El superadmin ve ambos en pantalla, con los KPI de fallos.
    let page = f.get(&f.sa_s, "/activity").await.body;
    assert!(page.contains("Acceso correcto") && page.contains("Acceso fallido") && page.contains("198.51.100.23"));
    assert_eq!(f.st.db.kpis().failed_logins_24h, 1);
}

#[tokio::test]
async fn activity_csv_export_is_safe_for_spreadsheets() {
    let f = fx();
    f.get(&f.user_s, "/sok?q=%3DSUM(1%2B1)").await;
    f.get(&f.user_s, "/sok?q=a%22b").await;
    let r = f.get(&f.sa_s, "/activity.csv").await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.headers.get(header::CONTENT_TYPE).unwrap().to_str().unwrap().starts_with("text/csv"));
    assert!(r.headers.get(header::CONTENT_DISPOSITION).unwrap().to_str().unwrap().contains("siffra-actividad.csv"));
    assert!(r.body.starts_with("\u{feff}time_utc,user,role,event,method,path,query,status,ip,detail,user_agent\r\n"));
    assert!(r.body.contains("\"'=SUM(1+1)\""), "las fórmulas se neutralizan: {}", r.body);
    assert!(r.body.contains("\"a\"\"b\""), "las comillas se duplican");
    // Exportar queda registrado.
    assert!(f.events().contains(&"export".to_string()));
}

#[tokio::test]
async fn activity_page_paginates() {
    let f = fx();
    for i in 0..60 {
        f.get(&f.user_s, &format!("/sok?q=busqueda{i}")).await;
    }
    let p1 = f.get(&f.sa_s, "/activity?event=search").await.body;
    assert!(p1.contains("Página 1 de 2") && p1.contains("busqueda59") && !p1.contains("busqueda0<"));
    let p2 = f.get(&f.sa_s, "/activity?event=search&page=2").await.body;
    assert!(p2.contains("Página 2 de 2") && p2.contains("busqueda0"));
}

// ───────────────────────── Idiomas ─────────────────────────

#[tokio::test]
async fn language_resolution_order() {
    let f = fx();
    // Sin nada: español.
    let r = f.get_anon("/login").await;
    assert!(r.body.contains(r#"<html lang="es""#) && r.body.contains("Iniciar sesión"));
    // Accept-Language.
    let r = send(&f.st, "GET", "/login", None, None, &[("accept-language", "sv-SE,sv;q=0.9,en;q=0.8")]).await;
    assert!(r.body.contains(r#"<html lang="sv""#) && r.body.contains("Logga in"));
    let r = send(&f.st, "GET", "/login", None, None, &[("accept-language", "en-US,en;q=0.9")]).await;
    assert!(r.body.contains(r#"<html lang="en""#) && r.body.contains("Sign in"));
    // La cookie gana a Accept-Language; ?lang= gana a la cookie y se recuerda.
    let r = send(&f.st, "GET", "/login", Some("lang=en"), None, &[("accept-language", "sv")]).await;
    assert!(r.body.contains("Sign in"));
    let r = send(&f.st, "GET", "/login?lang=sv", Some("lang=en"), None, &[]).await;
    assert!(r.body.contains("Logga in"));
    assert_eq!(r.cookie_value("lang").as_deref(), Some("sv"));
    // Un código desconocido se ignora.
    let r = f.get_anon("/login?lang=xx").await;
    assert!(r.body.contains("Iniciar sesión"));
    // El menú de idioma enlaza a las tres versiones conservando la ruta.
    let r = f.get(&f.user_s, "/sok?q=uppsala").await;
    for l in ["es", "en", "sv"] {
        assert!(r.body.contains(&format!(r#"href="/sok?q=uppsala&amp;lang={l}""#)), "falta el enlace a {l}");
    }
    for name in ["Español", "English", "Svenska"] {
        assert!(r.body.contains(name));
    }
}

#[tokio::test]
async fn language_choice_is_stored_in_the_profile_and_used_on_next_login() {
    let f = fx();
    f.get(&f.user_s, "/sok?lang=sv").await;
    assert_eq!(f.st.db.user_by_id(f.user.id).unwrap().lang, Lang::Sv);
    let r = f.login("testing@architechia.co", PW, "").await;
    assert_eq!(r.cookie_value("lang").as_deref(), Some("sv"), "al entrar se restaura su idioma");
    // Con la cookie del navegador de otra persona el perfil no manda: manda la cookie, luego el perfil.
    let s = session_from(&r);
    let page = send(&f.st, "GET", "/sok", Some(&format!("{}; lang=sv", s.cookie)), None, &[]).await;
    assert!(page.body.contains("Sök företag"));
}

#[tokio::test]
async fn pages_are_fully_translated_in_each_language() {
    let _env = lock_bolagsverket_env(); // pide una empresa inexistente: sin claves da 404, con otra prueba activa no
    let f = fx();
    let titles = [(Lang::Es, "Buscar empresas"), (Lang::En, "Search companies"), (Lang::Sv, "Sök företag")];
    for (lang, title) in titles {
        let r = f.get(&f.sa_s, &format!("/sok?lang={}", lang.code())).await;
        assert!(r.body.contains(&format!("<title>{title} — Siffra</title>")), "{lang:?}");
        // Ninguna frase de las otras lenguas se cuela en la vista.
        for (other, other_title) in titles {
            if other != lang {
                assert!(!r.body.contains(other_title), "{lang:?} muestra texto de {other:?}");
            }
        }
    }
    // Cada pantalla, en cada idioma, sin claves sin traducir.
    let uid = f.user.id;
    let paths = vec![
        "/sok".to_string(),
        "/sok?q=zzz".into(),
        "/sok?sort=revenue&dir=desc".into(),
        "/foretag/559012-3456".into(),
        "/foretag/559108-7721?tab=fin".into(),
        "/foretag/559108-7721?tab=ppl".into(),
        "/foretag/559234-9905?tab=ai".into(),
        "/foretag/559108-7721/benchmarks".into(),
        "/foretag/000000-0000".into(),
        "/bevakning".into(),
        "/likviditet".into(),
        "/sie".into(),
        "/fakturor".into(),
        "/profile".into(),
        "/users".into(),
        "/users/new".into(),
        format!("/users/{uid}/edit"),
        format!("/users/{uid}/delete"),
        "/activity".into(),
        "/no-existe".into(),
    ];
    for lang in Lang::ALL {
        for path in &paths {
            let sep = if path.contains('?') { '&' } else { '?' };
            let r = f.get(&f.sa_s, &format!("{path}{sep}lang={}", lang.code())).await;
            assert!(!r.body.contains('⟦'), "{lang:?} {path}: clave sin traducir en {}", r.body.split('⟦').nth(1).unwrap_or("").chars().take(40).collect::<String>());
            assert!(r.status == StatusCode::OK || r.status == StatusCode::NOT_FOUND, "{path} → {}", r.status);
        }
        let login = f.get_anon(&format!("/login?lang={}", lang.code())).await;
        assert!(!login.body.contains('⟦'), "login {lang:?}");
    }
}

#[tokio::test]
async fn error_and_form_messages_are_translated_too() {
    let f = fx();
    // Errores de formulario en sueco.
    let r = f.post(&f.sa_s, "/users?lang=sv", &[("name", "A"), ("email", "testing@architechia.co"), ("role", "user"), ("lang", "sv")]).await;
    assert!(r.body.contains("Det finns redan en användare med den e-postadressen"), "{}", r.body);
    let r = send(&f.st, "POST", "/login?lang=en", Some("siffra_pre=zz"), Some(form(&[("csrf", "zz"), ("identifier", "x"), ("password", "y")])), &[]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert!(r.body.contains("Wrong username or password"));
    // 403 en inglés.
    let r = f.get(&f.user_s, "/users?lang=en").await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(r.body.contains("Your role does not allow this action."));
}

#[test]
fn catalog_is_complete_and_consistent() {
    let mut seen = BTreeSet::new();
    for (key, es, en, sv) in ENTRIES {
        assert!(seen.insert(*key), "clave duplicada: {key}");
        for (lang, text) in [("es", es), ("en", en), ("sv", sv)] {
            assert!(!text.trim().is_empty(), "{key} sin texto en {lang}");
        }
        // Los mismos marcadores {0}, {1}… en las tres lenguas.
        let placeholders = |t: &str| -> BTreeSet<String> {
            (0..10).map(|i| format!("{{{i}}}")).filter(|p| t.contains(p.as_str())).collect()
        };
        assert_eq!(placeholders(es), placeholders(en), "{key}: marcadores es/en");
        assert_eq!(placeholders(es), placeholders(sv), "{key}: marcadores es/sv");
    }
}

#[test]
fn every_key_used_in_the_code_exists_in_the_catalog() {
    let prefixes: BTreeSet<&str> = ENTRIES.iter().filter_map(|(k, ..)| k.split('.').next()).collect();
    let sources = [
        ("views.rs", include_str!("views.rs")),
        ("views_admin.rs", include_str!("views_admin.rs")),
        ("handlers.rs", include_str!("handlers.rs")),
        ("auth.rs", include_str!("auth.rs")),
        ("app.rs", include_str!("app.rs")),
        ("model.rs", include_str!("model.rs")),
        ("summary.rs", include_str!("summary.rs")),
        ("db.rs", include_str!("db.rs")),
    ];
    let is_key_char = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.';
    let mut checked = 0;
    for (file, text) in sources {
        let bytes: Vec<char> = text.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == '"' {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && is_key_char(bytes[j]) {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == '"' && j > start {
                    let lit: String = bytes[start..j].iter().collect();
                    let first = lit.split('.').next().unwrap();
                    if lit.contains('.') && !lit.ends_with('.') && prefixes.contains(first) {
                        assert!(i18n::has_key(&lit), "{file}: la clave «{lit}» no existe en el catálogo");
                        checked += 1;
                    }
                }
            }
            i += 1;
        }
    }
    assert!(checked > 300, "se esperaban cientos de claves y solo se revisaron {checked}");
}

#[test]
fn every_activity_event_has_a_label_in_each_language() {
    for event in [
        "login_ok", "login_fail", "login_blocked", "logout", "view", "search", "company_view", "access_denied", "lang_change", "user_create",
        "user_update", "user_delete", "password_reset", "password_change", "password_change_fail", "profile_update", "export",
    ] {
        assert!(i18n::has_key(&format!("act.event.{event}")), "falta la etiqueta del evento {event}");
    }
}

// ───────────────────────── Pantallas de la aplicación (en sueco, como la original) ─────────────────────────

fn sv(path: &str) -> String {
    format!("{path}{}lang=sv", if path.contains('?') { '&' } else { '?' })
}

#[tokio::test]
async fn sok_lists_three_example_companies() {
    let f = fx();
    let r = f.get(&f.user_s, &sv("/sok")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("<title>Sök företag — Siffra</title>"));
    for name in ["Nordlys Logistik AB", "Fjällbruk Bygg &amp; Design AB", "Kvarn &amp; Krydda Livs AB"] {
        assert!(r.body.contains(name), "falta {name}");
    }
    assert!(r.body.contains("Låg risk") && r.body.contains("Hög risk") && r.body.contains("Bevaka"));
    assert!(r.body.contains("EXEMPEL"));
}

#[tokio::test]
async fn sok_filters_by_query() {
    let f = fx();
    let body = f.get(&f.user_s, &sv("/sok?q=uppsala")).await.body;
    let table = &body[body.find("<tbody").unwrap()..];
    assert!(table.contains("Kvarn &amp; Krydda Livs AB") && !table.contains("Nordlys Logistik AB"));
    assert!(f.get(&f.user_s, &sv("/sok?q=zzz")).await.body.contains("Inga resultat"));
}

#[tokio::test]
async fn company_tabs_render() {
    let f = fx();
    let r = f.get(&f.user_s, &sv("/foretag/559108-7721")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("Omsättning (tkr)"));
    assert!(f.get(&f.user_s, &sv("/foretag/559108-7721?tab=fin")).await.body.contains("Årets resultat"));
    assert!(f.get(&f.user_s, &sv("/foretag/559108-7721?tab=ppl")).await.body.contains("Eva Testlund"));
    let ai = f.get(&f.user_s, &sv("/foretag/559108-7721?tab=ai")).await.body;
    assert!(ai.contains("Sammanfattning") && ai.contains("Varning. Företaget har gått med förlust"));
    // En español la ficha usa las unidades y los separadores del idioma.
    let es = f.get(&f.user_s, "/foretag/559012-3456?lang=es").await.body;
    assert!(es.contains("64.100 mil SEK") && es.contains("57,7"), "{es}");
    let en = f.get(&f.user_s, "/foretag/559012-3456?lang=en").await.body;
    assert!(en.contains("64,100 kSEK") && en.contains("57.7%"));
}

#[tokio::test]
async fn overview_falls_back_to_example_medians_without_scb() {
    let f = fx();
    let body = f.get(&f.user_s, &sv("/foretag/559012-3456")).await.body;
    assert!(body.contains("Branschmedianer (SNI 52.290)"));
    assert!(!body.contains("källa: SCB"));
}

#[tokio::test]
async fn unknown_company_is_404() {
    // Sin claves de Bolagsverket no se consulta a nadie: un número desconocido es 404.
    let _env = lock_bolagsverket_env();
    let f = fx();
    let r = f.get(&f.user_s, &sv("/foretag/000000-0000")).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert!(r.body.contains("Företaget hittades inte"));
}

#[tokio::test]
async fn static_pages_render() {
    let f = fx();
    for (path, needle) in [
        ("/likviditet", "Likviditetsprognos"),
        ("/sie", "Släpp en SIE-fil här"),
        ("/fakturor", "Hamnkraft Test AB"),
    ] {
        let r = f.get(&f.user_s, &sv(path)).await;
        assert_eq!(r.status, StatusCode::OK, "{path}");
        assert!(r.body.contains(needle), "{path} no contiene {needle:?}");
    }
}

#[tokio::test]
async fn sok_sorts_by_revenue_and_marks_aria_sort() {
    let f = fx();
    // Solo dentro de la tabla (el placeholder del buscador también podría mencionar un nombre).
    let pos = |body: &str, name: &str| {
        let table = &body[body.find("<tbody").expect("tbody")..];
        table.find(name).unwrap_or_else(|| panic!("falta {name}"))
    };
    let asc = f.get(&f.user_s, &sv("/sok?sort=revenue&dir=asc")).await.body;
    assert!(pos(&asc, "Kvarn") < pos(&asc, "Nordlys") && pos(&asc, "Nordlys") < pos(&asc, "Fjällbruk"));
    assert!(asc.contains(r#"aria-sort="ascending""#));
    let desc = f.get(&f.user_s, &sv("/sok?sort=revenue&dir=desc")).await.body;
    assert!(pos(&desc, "Fjällbruk") < pos(&desc, "Nordlys") && pos(&desc, "Nordlys") < pos(&desc, "Kvarn"));
    assert!(desc.contains(r#"aria-sort="descending""#));
    let other = f.get(&f.user_s, &sv("/sok?sort=nope")).await.body;
    assert!(!other.contains(r#"aria-sort=""#));
}

#[tokio::test]
async fn pages_have_accessible_structure() {
    let f = fx();
    let body = f.get(&f.user_s, &sv("/sok")).await.body;
    assert!(body.contains(r##"href="#main""##), "enlace para saltar al contenido");
    assert_eq!(body.matches("<h1").count(), 1, "un único h1");
    assert!(body.contains(r#"aria-label="Huvudmeny""#));
    assert!(body.contains(r#"aria-current="page""#));
    assert!(!body.contains(r#"aria-current="false""#));
    let co = f.get(&f.user_s, &sv("/foretag/559012-3456?tab=fin")).await.body;
    assert!(co.contains(r#"aria-label="Företagsvyer""#));
    assert!(!co.contains(r#"role="tab""#));
    assert!(co.contains("<caption"));
    // Formularios con etiquetas asociadas.
    let login = f.get_anon("/login").await.body;
    assert!(login.contains(r#"for="identifier""#) && login.contains(r#"autocomplete="current-password""#));
}

#[tokio::test]
async fn benchmark_fragment_route() {
    let f = fx();
    let r = f.get(&f.user_s, &sv("/foretag/559108-7721/benchmarks")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("Nettomarginal") && r.body.contains("Kassalikviditet"));
    assert!(!r.body.contains("<html"), "es un fragmento, no un documento");
    assert_eq!(f.get(&f.user_s, "/foretag/000000-0000/benchmarks").await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn interactive_elements_expose_tooltips() {
    let f = fx();
    let co = f.get(&f.user_s, &sv("/foretag/559108-7721")).await.body;
    assert_eq!(co.matches(r#"class="bar-group""#).count(), 5);
    assert!(co.contains("mot 2023"), "la barra de 2024 compara con 2023");
    assert!(co.contains(r#"data-copy="559108-7721""#));
    assert!(co.contains(r#"class="term""#) && co.contains("Soliditet"));
    assert!(co.contains(r#"class="info""#));
    assert_eq!(co.matches(r#"tabindex="0""#).count(), 5 + 3, "5 barras + 3 pistas de comparación, alcanzables con Tab");
    let cash = f.get(&f.user_s, &sv("/likviditet")).await.body;
    assert_eq!(cash.matches(r#"class="bar-group""#).count(), 13);
    assert!(cash.contains("Under gränsen"));
    assert!(cash.contains(r#"class="threshold-group""#));
    let sok = f.get(&f.user_s, &sv("/sok")).await.body;
    assert!(sok.contains("Sortera efter företag (stigande)"));
    assert!(sok.contains("<kbd>/</kbd>"));
    assert!(!sok.contains(" title="), "los tooltips no se duplican con title nativo");
}

#[test]
fn pending_benchmark_renders_skeleton_with_fragment_url() {
    let html = views::company_page(&Ctx::new(Lang::Sv), &model::EXAMPLE_COMPANIES[0], "ov", &views::Bench::Pending).into_string();
    assert!(html.contains(r#"data-fragment="/foretag/559012-3456/benchmarks""#));
    assert!(html.contains(r#"aria-busy="true""#));
    assert!(html.contains("skeleton-row"));
    assert!(html.contains("<noscript>"));
}

#[tokio::test]
async fn serves_stylesheet() {
    let f = fx();
    let r = f.get_anon("/static/styles.css").await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.headers.get(header::CONTENT_TYPE).unwrap().to_str().unwrap().starts_with("text/css"));
    assert!(r.body.contains("--accent") && r.body.contains("backdrop-filter"), "estilo de vidrio líquido");
    assert!(r.body.contains("prefers-reduced-motion") && r.body.contains("prefers-reduced-transparency"));
}

// ───────────────────────── Bolagsverket simulado ─────────────────────────

/// Flujo completo contra un Bolagsverket SIMULADO en local (OAuth + `/organisationer` con la forma del
/// ejemplo oficial). No toca la red real. Un solo test porque modifica variables de entorno.
#[tokio::test]
async fn live_bolagsverket_flow_with_fake_api() {
    use axum::http::HeaderMap;
    use axum::{Form, Json};
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
    fn authorised(headers: &HeaderMap) -> bool {
        headers.get("authorization").and_then(|v| v.to_str().ok()) == Some("Bearer tok-123")
    }
    // Lista de cuentas anuales: tres informes (2025, 2024 y 2023) para 5299999994; ninguno para el resto.
    async fn fake_dokumentlista(headers: HeaderMap, Json(body): Json<Value>) -> Response {
        if !authorised(&headers) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        let docs = if body["identitetsbeteckning"].as_str() == Some("5299999994") {
            json!([
                {"dokumentId": "doc-2025", "filformat": "application/zip", "rapporteringsperiodTom": "2025-12-31", "registreringstidpunkt": "2026-07-01"},
                {"dokumentId": "doc-2024", "filformat": "application/zip", "rapporteringsperiodTom": "2024-12-31", "registreringstidpunkt": "2025-07-01"},
                {"dokumentId": "doc-2023", "filformat": "application/zip", "rapporteringsperiodTom": "2023-12-31", "registreringstidpunkt": "2024-07-01"}
            ])
        } else {
            json!([])
        };
        Json(json!({ "dokument": docs })).into_response()
    }
    // Cada documento es un ZIP real con un informe iXBRL (sintético, con la estructura observada en producción).
    async fn fake_dokument(headers: HeaderMap, axum::extract::Path(id): axum::extract::Path<String>) -> Response {
        use std::io::Write;
        if !authorised(&headers) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        let year = match id.as_str() {
            "doc-2025" => 2025,
            "doc-2023" => 2023,
            _ => return StatusCode::NOT_FOUND.into_response(),
        };
        let xhtml = annual_report::tests::report_xml(year, 1_000_000);
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            w.start_file("informe.xhtml", opts).unwrap();
            w.write_all(xhtml.as_bytes()).unwrap();
            w.finish().unwrap();
        }
        ([(header::CONTENT_TYPE, "application/zip")], buf.into_inner()).into_response()
    }

    let fake = Router::new()
        .route("/oauth2/token", post(fake_token))
        .route("/vardefulla-datamangder/v1/organisationer", post(fake_orgs))
        .route("/vardefulla-datamangder/v1/dokumentlista", post(fake_dokumentlista))
        .route("/vardefulla-datamangder/v1/dokument/:id", get(fake_dokument));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, fake).await.unwrap() });

    std::env::set_var("BOLAGSVERKET_CLIENT_ID", "test-id");
    std::env::set_var("BOLAGSVERKET_CLIENT_SECRET", "test-secret");
    std::env::set_var("BOLAGSVERKET_BASE_URL", format!("http://{addr}/vardefulla-datamangder/v1"));

    let f = fx();
    let live = |path: &str| {
        let url = sv(path);
        let (st, cookie) = (f.st.clone(), f.user_s.cookie.clone());
        async move { send(&st, "GET", &url, Some(&cookie), None, &[]).await }
    };

    let r = live("/foretag/5299999994").await;
    assert_eq!(r.status, StatusCode::OK);
    for needle in [
        "Cykelbolaget AB",
        "529999-9994",
        "Avregistrerat 2023-05-05",
        "Konkurs (sedan 2024-01-26)",
        "Jobbstigen 2, 12345 Grönköping",
        "Bedriva handel med cyklar och tillbehör till cyklar",
        "Källa: Bolagsverket (gratis API för värdefulla datamängder)",
    ] {
        assert!(r.body.contains(needle), "la ficha real no contiene {needle:?}");
    }
    // Cuentas anuales: la ficha sale al instante con un esqueleto y el fragmento trae las cifras reales.
    assert!(r.body.contains(r#"data-fragment="/foretag/5299999994/bokslut""#), "esqueleto de carga diferida");
    let frag = live("/foretag/5299999994/bokslut").await;
    assert_eq!(frag.status, StatusCode::OK);
    assert!(!frag.body.contains("<html"), "es un fragmento");
    for needle in [
        "Omsättning 2025",
        "1\u{00A0}002 tkr", // 1 002 025 coronas → 1 002 tkr (gana el hecho exacto, no el redondeado)
        "−46 tkr",          // resultado 2025: −45 678 coronas, con sign="-"
        "Soliditet",
        "33,3\u{00A0}%", // 300 / 900
        "Eget kapital",
        "Summa tillgångar",
        "senaste räkenskapsår slutade 2025-12-31",
        "Finansiell bedömning",
        "Baserat på räkenskapsåret 2025",
        "Hög risk",
        "Pågående förfarande enligt registret: Konkurs (sedan 2024-01-26)",
        "Resultatet blev en förlust på 46 tkr",
    ] {
        assert!(frag.body.contains(needle), "el fragmento no contiene {needle:?}");
    }
    // Se leen informes alternos (2025 y 2023) y se unen: 2022..2025 → cuatro años en la tabla.
    for year in ["2022", "2023", "2024", "2025"] {
        assert!(frag.body.contains(&format!("<th class=\"right\" scope=\"col\">{year}</th>")), "falta la columna {year}");
    }
    // Con las cifras ya en caché, la ficha las incluye directamente (sin esqueleto).
    let again = live("/foretag/5299999994").await;
    assert!(!again.body.contains("data-fragment"), "ya en caché: sin carga diferida");
    assert!(again.body.contains("Omsättning 2025") && again.body.contains("Årsredovisning"));
    // Una empresa real sin cuentas digitales: mensaje claro en vez de cifras.
    let sample_org = bolagsverket::map_organisation(&serde_json::from_str::<Value>(bolagsverket::fixtures::AKTIEBOLAG).unwrap(), "5299999994").unwrap();
    let none = views::financials_fragment(&Ctx::new(Lang::Sv), &sample_org, None, None).into_string();
    assert!(none.contains("inte lämnat in någon digital årsredovisning"));
    // Esta empresa de prueba está en konkurs: aunque no haya cuentas, el procedimiento la deja en riesgo alto.
    assert!(none.contains("Hög risk") && none.contains("Pågående förfarande enligt registret: Konkurs (sedan 2024-01-26)"), "{none}");
    assert!(none.contains("Ingen digital årsredovisning"));
    // Una de EJEMPLO no pasa por este fragmento.
    assert_eq!(live("/foretag/559012-3456/bokslut").await.status, StatusCode::NOT_FOUND);
    // Buscador: un organisationsnummer real que no es de ejemplo.
    let sok = live("/sok?q=529999-9994").await;
    assert!(sok.body.contains("Cykelbolaget AB") && sok.body.contains("Bolagsverket"));
    assert!(sok.body.contains("1 träff"));
    // No existe → 404; fuente de datos caída o error HTTP del API → 502 con página de error.
    assert_eq!(live("/foretag/5560000001").await.status, StatusCode::NOT_FOUND);
    // Dígito de control inválido: se rechaza antes de llamar a la API (404, no 502).
    assert_eq!(live("/foretag/5560000009").await.status, StatusCode::NOT_FOUND);
    assert_eq!(live("/foretag/5560000019").await.status, StatusCode::BAD_GATEWAY);
    assert_eq!(live("/foretag/5560000027").await.status, StatusCode::BAD_GATEWAY);
    // Un personnummer (12 dígitos) nunca se envía al API.
    assert_eq!(live("/foretag/194009272719").await.status, StatusCode::NOT_FOUND);
    // Las empresas de EJEMPLO siguen sirviéndose igual.
    assert_eq!(live("/foretag/559012-3456").await.status, StatusCode::OK);
    // La actividad registra la consulta de la empresa real con su número normalizado.
    let (rows, _) = f.st.db.activity(&ActivityFilter { event: Some("company_view".into()), ..Default::default() }, 50, 0);
    assert!(rows.iter().any(|r| r.detail == "5299999994"));

    std::env::remove_var("BOLAGSVERKET_CLIENT_ID");
    std::env::remove_var("BOLAGSVERKET_CLIENT_SECRET");
    std::env::remove_var("BOLAGSVERKET_BASE_URL");
}

// ───────────────────────── Búsqueda por nombre en el registro ─────────────────────────

/// Pone un índice de registro (con `data` en el formato del archivo de Bolagsverket) en el estado de la app.
fn with_registry(f: &mut Fx, data: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("siffra-reg-{}", crate::util::random_hex(4)));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("registry.db");
    crate::registry::import(data.as_bytes(), &path, None, "prueba", 8_000, |_| {}).unwrap();
    f.st.registry = crate::registry::RegistryHandle::new(path.clone());
    path
}

#[tokio::test]
async fn name_search_lists_registry_companies_and_links_only_real_organisation_numbers() {
    let mut f = fx();
    let path = with_registry(&mut f, crate::registry::tests::SAMPLE);
    // Sociedad: enlace a su ficha, forma jurídica traducida, número con guion.
    let r = f.get(&f.user_s, "/sok?q=fjallbruk&lang=es").await.body;
    assert!(r.contains(r#"href="/foretag/5560000027""#), "{r}");
    assert!(r.contains("556000-0027") && r.contains("Sociedad anónima (AB)") && r.contains("ÖSTERSUND"));
    assert!(r.contains("Resultados del registro de empresas de Bolagsverket"));
    assert!(r.contains("1 resultado para «fjallbruk»"));
    // Persona física (identidad de 12 dígitos, dada de baja): se muestra pero no es un enlace, y se atenúa.
    let p = f.get(&f.user_s, "/sok?q=ostlund&lang=es").await.body;
    assert!(p.contains("Åsa Östlund") && p.contains("Dada de baja el 2010-10-14") && p.contains("Empresa individual"));
    assert!(!p.contains("/foretag/199001019999"), "una identidad que no es organisationsnummer no tiene ficha");
    assert!(p.contains(r#"class="dim""#));
    // Mismas filas en sueco, con su vocabulario.
    let sv = f.get(&f.user_s, "/sok?q=fjallbruk&lang=sv").await.body;
    assert!(sv.contains("Aktiebolag") && sv.contains("1 träff för"));
    // Sin coincidencias: estado vacío de siempre.
    assert!(f.get(&f.user_s, "/sok?q=zzzzz&lang=es").await.body.contains("Sin resultados"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[tokio::test]
async fn name_search_is_off_without_an_index_and_never_breaks_the_example_search() {
    let f = fx();
    let r = f.get(&f.user_s, "/sok?q=fjallbruk&lang=es").await.body;
    assert!(r.contains("Sin resultados"), "sin índice no hay resultados por nombre");
    assert!(!r.contains("Resultados del registro"));
    // Las empresas de ejemplo siguen saliendo igual.
    assert!(f.get(&f.user_s, "/sok?q=nordlys&lang=es").await.body.contains("Nordlys Logistik AB"));
}

#[tokio::test]
async fn name_search_caps_the_list_and_says_so() {
    let mut f = fx();
    let mut data = String::from("organisationsidentitet;namnskyddslopnummer;registreringsland;organisationsnamn;organisationsform;avregistreringsdatum;avregistreringsorsak;pagandeAvvecklingsEllerOmstruktureringsforfarande;registreringsdatum;verksamhetsbeskrivning;postadress\n");
    for i in 0..40 {
        data.push_str(&format!("\"55600{i:05}$ORGNR-IDORG\";\"1\";\"SE-LAND\";\"Bolag {i} AB$FORETAGSNAMN-ORGNAM$2000-01-01\";\"AB-ORGFO\";\"\";\"\";\"\";\"2000-01-01\";\"\";\"Gatan 1$$MALMÖ$21100$SE-LAND\"\n"));
    }
    let path = with_registry(&mut f, &data);
    let r = f.get(&f.user_s, "/sok?q=bolag&lang=es").await.body;
    assert_eq!(r.matches("/foretag/55600").count(), 25, "máximo 25 filas");
    assert!(r.contains("Se muestran los primeros 25 resultados"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[tokio::test]
async fn name_search_page_has_no_missing_translations() {
    let mut f = fx();
    let path = with_registry(&mut f, crate::registry::tests::SAMPLE);
    for lang in Lang::ALL {
        for q in ["fjallbruk", "ostlund", "zzzzz"] {
            let r = f.get(&f.sa_s, &format!("/sok?q={q}&lang={}", lang.code())).await;
            assert!(!r.body.contains('⟦'), "{lang:?} {q}");
        }
    }
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[tokio::test]
async fn weekly_refresh_downloads_imports_and_swaps_the_index() {
    use std::io::Write;
    // Servidor local que sirve un zip con el archivo (como el de Bolagsverket) y otra ruta que da error.
    async fn bulk() -> Response {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            w.start_file("bolagsverket_bulkfil.txt", opts).unwrap();
            w.write_all(crate::registry::tests::SAMPLE.as_bytes()).unwrap();
            w.finish().unwrap();
        }
        ([(header::CONTENT_TYPE, "application/zip")], buf.into_inner()).into_response()
    }
    async fn broken() -> StatusCode {
        StatusCode::INTERNAL_SERVER_ERROR
    }
    let app = Router::new().route("/bulk.zip", get(bulk)).route("/roto.zip", get(broken));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let dir = std::env::temp_dir().join(format!("siffra-refresh-{}", crate::util::random_hex(4)));
    let handle = crate::registry::RegistryHandle::new(dir.join("registry.db"));
    assert!(handle.get().is_none() && handle.age().is_none(), "al principio no hay índice");

    // Un fallo de descarga no cambia nada.
    let err = crate::registry::refresh(&handle, &format!("http://{addr}/roto.zip"), 0).await.unwrap_err();
    assert!(err.contains("500"), "{err}");
    assert!(handle.get().is_none());
    // Una descarga demasiado pequeña (truncada) se rechaza.
    let err = crate::registry::refresh(&handle, &format!("http://{addr}/bulk.zip"), 10_000_000).await.unwrap_err();
    assert!(err.contains("más pequeña"), "{err}");
    assert!(handle.get().is_none());

    let report = crate::registry::refresh(&handle, &format!("http://{addr}/bulk.zip"), 0).await.unwrap();
    assert_eq!((report.read, report.inserted), (3, 3));
    let reg = handle.get().expect("índice en servicio");
    assert_eq!(reg.search("fjallbruk", 5, true).len(), 1);
    assert!(handle.age().unwrap() < std::time::Duration::from_secs(60), "recién renovado");
    assert!(!dir.join("bulk/bolagsverket_bulkfil.zip").exists(), "el zip se borra tras importar");
    assert!(!dir.join("bulk/bolagsverket_bulkfil.zip.part").exists());

    // Segunda renovación: el índice anterior sigue respondiendo y luego se sustituye. Solo en Unix (el servidor):
    // Windows no deja reemplazar un archivo que está abierto.
    #[cfg(unix)]
    {
        let again = crate::registry::refresh(&handle, &format!("http://{addr}/bulk.zip"), 0).await.unwrap();
        assert_eq!(again.inserted, 3);
        assert_eq!(handle.get().unwrap().stats().rows, 3);
        assert_eq!(reg.stats().rows, 3, "quien ya tenía el índice anterior no se ve afectado");
    }
    drop(reg);
    let _ = std::fs::remove_dir_all(&dir);
}

// ───────────────────────── Modo real (sin datos de ejemplo) ─────────────────────────

/// Como `fx()` pero con el modo demo apagado, que es lo normal en producción.
fn fx_real() -> Fx {
    let mut f = fx();
    f.st.demo = false;
    f
}

#[tokio::test]
async fn without_demo_mode_there_are_no_fictional_companies_or_screens() {
    let f = fx_real();
    // Portada del buscador: invitación a buscar y empresas reales conocidas, sin lista ni nota de ejemplo.
    let r = f.get(&f.user_s, "/sok?lang=es").await;
    assert_eq!(r.status, StatusCode::OK);
    for needle in ["Busca una empresa sueca", "Prueba con:", "Volvo", "Spotify", "Los datos son reales"] {
        assert!(r.body.contains(needle), "falta {needle:?}");
    }
    for banned in ["Nordlys", "Fjällbruk", "Kvarn", "EJEMPLO", "Datos de demostración", "<tbody"] {
        assert!(!r.body.contains(banned), "no debería aparecer {banned:?}");
    }
    // El menú lleva el buscador y las pantallas reales (Mis empresas, Comparar, Historial); las maquetas no.
    for real in ["/sok", "/bevakning", "/comparar", "/historial"] {
        assert!(r.body.contains(&format!(r#"href="{real}""#)), "{real} falta en el menú");
    }
    for hidden in ["/likviditet", "/sie", "/fakturor"] {
        assert!(!r.body.contains(&format!(r#"href="{hidden}""#)), "{hidden} sigue en el menú");
        assert_eq!(f.get(&f.user_s, hidden).await.status, StatusCode::NOT_FOUND, "{hidden}");
    }
    // Las empresas de ejemplo no existen: ni la ficha ni sus fragmentos.
    assert_eq!(f.get(&f.user_s, "/foretag/559012-3456").await.status, StatusCode::NOT_FOUND);
    assert_eq!(f.get(&f.user_s, "/foretag/559012-3456/benchmarks").await.status, StatusCode::NOT_FOUND);
    // Buscar un nombre de ejemplo no devuelve nada, y el estado vacío propone búsquedas reales.
    let q = f.get(&f.user_s, "/sok?q=nordlys&lang=es").await.body;
    assert!(q.contains("Sin resultados") && !q.contains("Nordlys Logistik"));
    assert!(q.contains(r#"href="/sok?q=Volvo""#));
    // En modo demo, en cambio, todo sigue ahí.
    let demo = fx();
    assert!(demo.get(&demo.user_s, "/sok?q=nordlys").await.body.contains("Nordlys Logistik AB"));
    assert_eq!(demo.get(&demo.user_s, "/bevakning").await.status, StatusCode::OK);
}

#[tokio::test]
async fn real_mode_pages_have_no_missing_translations_and_search_by_name_still_works() {
    let mut f = fx_real();
    let path = with_registry(&mut f, crate::registry::tests::SAMPLE);
    for lang in Lang::ALL {
        for url in ["/sok", "/sok?q=fjallbruk", "/sok?q=zzzzz", "/no-existe"] {
            let sep = if url.contains('?') { '&' } else { '?' };
            let r = f.get(&f.sa_s, &format!("{url}{sep}lang={}", lang.code())).await;
            assert!(!r.body.contains('⟦'), "{lang:?} {url}");
        }
    }
    let r = f.get(&f.user_s, "/sok?q=fjallbruk&lang=es").await.body;
    assert!(r.contains(r#"href="/foretag/5560000027""#) && r.contains("Resultados del registro de empresas"));
    assert!(!r.contains("Datos de demostración"), "sin modo demo no hay nota de ejemplo");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[tokio::test]
async fn demo_flag_is_off_unless_asked_for() {
    let st = AppState::new(Db::memory());
    assert!(!st.demo, "producción no debe arrancar con datos ficticios");
}

#[test]
fn assessment_numbers_are_rounded_and_the_summary_reads_well() {
    use crate::annual_report::{FinancialYear, Financials};
    use crate::scb::SectorMedians;
    let mut org = bolagsverket::map_organisation(&serde_json::from_str::<serde_json::Value>(bolagsverket::fixtures::AKTIEBOLAG).unwrap(), "5299999994").unwrap();
    org.forfaranden.clear();
    org.avregistreringsdatum = None;
    org.aktiv = true;
    let year = |label: &str, revenue, result, equity, assets| FinancialYear { period_end: format!("{label}-12-31"), label: label.into(), revenue: Some(revenue), result: Some(result), equity: Some(equity), assets: Some(assets) };
    let fin = Financials { years: vec![year("2024", 2_615, 190, 580, 2_100), year("2025", 2_519, 176, 600, 2_150)] };
    let med = SectorMedians { margin: 18.2, solidity: 68.0, liquidity: 294.0, year: "2024".into(), sni_code: "71.121".into(), sni_label: "Tekniska konsultbyråer".into(), size_class: "TOT".into(), exact_sni: true, exact_size: false };

    let es = views::financials_fragment(&Ctx::new(Lang::Es), &org, Some(&fin), Some(&med)).into_string();
    assert!(!es.contains("6,98") && !es.contains("27,90"), "las barras no enseñan decimales de más");
    assert!(es.contains("La solidez es del 27,9\u{a0}%, por debajo de la mediana del sector (68,0\u{a0}%)."), "gramática del resumen en español");
    assert!(es.contains("Riesgo bajo"), "27,9 % de solidez y 7 % de margen no son débiles por sí solos: sin alerta");
    assert!(es.contains("En 2025 la facturación fue de 2.519 mil SEK, un 3,7\u{a0}% menos que en 2024."));
    let sv = views::financials_fragment(&Ctx::new(Lang::Sv), &org, Some(&fin), Some(&med)).into_string();
    assert!(sv.contains("Soliditeten är 27,9\u{a0}%, under branschens median (68,0\u{a0}%)."));
    assert!(sv.contains("Låg risk"));
}

// ───────────────────────── Mis empresas, Comparar, Historial ─────────────────────────

/// (Comparte token y credenciales con la prueba del flujo completo: el token se guarda en caché en todo el proceso.)
/// Bolagsverket simulado: sirve la empresa de ejemplo del API oficial (en konkurs) para cualquiera de los dos
/// números y ninguna cuenta anual. Devuelve su dirección; hay que llevar el cerrojo de las variables BOLAGSVERKET_*.
async fn fake_bolagsverket() -> std::net::SocketAddr {
    use axum::http::HeaderMap;
    use axum::{Form, Json};
    use serde_json::{json, Value};
    use std::collections::HashMap;

    async fn token(Form(_): Form<HashMap<String, String>>) -> Response {
        Json(json!({ "access_token": "tok-123", "expires_in": 3600 })).into_response()
    }
    async fn orgs(headers: HeaderMap, Json(body): Json<Value>) -> Response {
        if headers.get("authorization").and_then(|v| v.to_str().ok()) != Some("Bearer tok-123") {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        match body["identitetsbeteckning"].as_str() {
            Some("5567037485" | "5560125790" | "5569000010" | "5569000028") => Json(serde_json::from_str::<Value>(bolagsverket::fixtures::AKTIEBOLAG).unwrap()).into_response(),
            _ => Json(serde_json::from_str::<Value>(bolagsverket::fixtures::FINNS_EJ).unwrap()).into_response(),
        }
    }
    async fn docs(Json(_): Json<Value>) -> Response {
        Json(json!({ "dokument": [] })).into_response()
    }
    let app = Router::new()
        .route("/oauth2/token", post(token))
        .route("/vardefulla-datamangder/v1/organisationer", post(orgs))
        .route("/vardefulla-datamangder/v1/dokumentlista", post(docs));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    std::env::set_var("BOLAGSVERKET_CLIENT_ID", "test-id");
    std::env::set_var("BOLAGSVERKET_CLIENT_SECRET", "test-secret");
    std::env::set_var("BOLAGSVERKET_BASE_URL", format!("http://{addr}/vardefulla-datamangder/v1"));
    addr
}

fn unset_bolagsverket_env() {
    for k in ["BOLAGSVERKET_CLIENT_ID", "BOLAGSVERKET_CLIENT_SECRET", "BOLAGSVERKET_BASE_URL"] {
        std::env::remove_var(k);
    }
}

#[tokio::test]
async fn my_companies_follow_review_change_and_unfollow() {
    let _env = lock_bolagsverket_env();
    fake_bolagsverket().await;
    let f = fx();
    let org = "5569000010";

    // Vacía al principio, con su invitación.
    let r = f.get(&f.user_s, "/bevakning?lang=es").await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("Aún no sigues ninguna empresa"));

    // La ficha ofrece seguirla; seguir exige el token CSRF.
    let ficha = f.get(&f.user_s, &format!("/foretag/{org}?lang=es")).await.body;
    assert!(ficha.contains("Seguir empresa") && ficha.contains(r#"action="/bevakning/add""#));
    let no_csrf = f.post_no_csrf(&f.user_s, "/bevakning/add", &[("orgnr", org)]).await;
    assert_eq!(no_csrf.status, StatusCode::FORBIDDEN);
    assert_eq!(f.st.db.watch_count(f.user.id), 0);

    let added = f.post(&f.user_s, "/bevakning/add", &[("orgnr", org), ("next", &format!("/foretag/{org}"))]).await;
    assert_eq!((added.status, added.location().as_deref()), (StatusCode::SEE_OTHER, Some("/foretag/5569000010")));
    assert!(f.st.db.watch_has(f.user.id, org));
    // Seguirla dos veces no la duplica.
    f.post(&f.user_s, "/bevakning/add", &[("orgnr", org)]).await;
    assert_eq!(f.st.db.watch_count(f.user.id), 1);
    let ficha = f.get(&f.user_s, &format!("/foretag/{org}?lang=es")).await.body;
    assert!(ficha.contains("Dejar de seguir") && ficha.contains(r#"action="/bevakning/remove""#));

    // En la lista: nombre real, enlace, y "pendiente" hasta la primera revisión.
    let list = f.get(&f.user_s, "/bevakning?lang=es").await.body;
    assert!(list.contains("Cykelbolaget AB") && list.contains(&format!(r#"href="/foretag/{org}""#)) && list.contains("556900-0010"));
    assert!(list.contains("Pendiente de revisar"));
    // Cada persona ve solo las suyas.
    assert!(f.get(&f.admin_s, "/bevakning?lang=es").await.body.contains("Aún no sigues ninguna empresa"));

    // Revisión: la empresa está en konkurs → riesgo alto y estado "En procedimiento".
    let refreshed = f.post(&f.user_s, "/bevakning/refresh", &[("orgnr", "all")]).await;
    assert_eq!(refreshed.location().as_deref(), Some("/bevakning?ok=refreshed&n=1"));
    let list = f.get(&f.user_s, "/bevakning?ok=refreshed&n=1&lang=es").await.body;
    assert!(list.contains("Se actualizaron 1 empresas.") && list.contains("Riesgo alto") && list.contains("En procedimiento"), "{list}");
    assert!(!list.contains("Pendiente de revisar"));

    // Un cambio de riesgo entre revisiones se marca con de-a.
    f.st.db.watch_apply(org, &crate::db::WatchSnapshot { name: "Cykelbolaget AB".into(), form: "Aktiebolag".into(), level: "good".into(), year: None, revenue: None, result: None, solidity: None, flags: 3 });
    f.post(&f.user_s, "/bevakning/refresh", &[("orgnr", org)]).await;
    let list = f.get(&f.user_s, "/bevakning?lang=es").await.body;
    assert!(list.contains("Riesgo: Riesgo bajo → Riesgo alto"), "el cambio de good a bad se muestra: {list}");
    assert!(list.contains(r#"class="change new worse""#), "reciente y a peor");
    let en = f.get(&f.user_s, "/bevakning?lang=en").await.body;
    assert!(en.contains("Risk: Low risk → High risk"));

    // Dejar de seguir.
    let nocsrf = f.post_no_csrf(&f.user_s, "/bevakning/remove", &[("orgnr", org)]).await;
    assert_eq!(nocsrf.status, StatusCode::FORBIDDEN);
    let removed = f.post(&f.user_s, "/bevakning/remove", &[("orgnr", org), ("next", "/bevakning")]).await;
    assert_eq!(removed.location().as_deref(), Some("/bevakning?ok=removed"));
    assert!(!f.st.db.watch_has(f.user.id, org));
    let events = f.events();
    for e in ["watch_add", "watch_remove", "watch_refresh"] {
        assert!(events.contains(&e.to_string()), "{e} registrado en la actividad");
    }
    unset_bolagsverket_env();
}

#[tokio::test]
async fn following_is_capped_and_validates_the_number() {
    let _env = lock_bolagsverket_env();
    fake_bolagsverket().await;
    let f = fx();
    for i in 0..crate::db::WATCH_LIMIT {
        f.st.db.watch_add(f.user.id, &format!("55600{i:05}"), "Relleno AB", "");
    }
    let r = f.post(&f.user_s, "/bevakning/add", &[("orgnr", "5569000028")]).await;
    assert_eq!(r.location().as_deref(), Some("/bevakning?err=limit"));
    assert!(!f.st.db.watch_has(f.user.id, "5569000028"));
    assert!(f.get(&f.user_s, "/bevakning?err=limit&lang=es").await.body.contains("Puedes seguir hasta 50 empresas"));
    // Un número que no es de organización no se guarda.
    let bad = f.post(&f.sa_s, "/bevakning/add", &[("orgnr", "123")]).await;
    assert_eq!(bad.status, StatusCode::NOT_FOUND);
    assert_eq!(f.st.db.watch_count(f.sa.id), 0);
    unset_bolagsverket_env();
}

#[tokio::test]
async fn compare_shows_companies_side_by_side_and_explains_bad_input() {
    let _env = lock_bolagsverket_env();
    fake_bolagsverket().await;
    let f = fx();
    // Dos empresas válidas: columnas con sus nombres y números, y las filas de datos.
    let r = f.get(&f.user_s, "/comparar?o=556703-7485&o=5560125790&o=&o=&lang=es").await;
    assert_eq!(r.status, StatusCode::OK);
    for needle in ["556703-7485", "556012-5790", "Cykelbolaget AB", "Forma jurídica", "Estado", "Riesgo", "Solidez", "Margen neto", "Último ejercicio", "Crecimiento de la facturación", "Mediana del sector", "El valor más alto de cada fila"] {
        assert!(r.body.contains(needle), "falta {needle:?}");
    }
    assert!(r.body.contains("Riesgo alto"), "ambas están en konkurs");
    // Con una sola empresa o con basura, un aviso claro y sin tabla.
    let one = f.get(&f.user_s, "/comparar?o=556703-7485&lang=es").await.body;
    assert!(one.contains("Indica al menos dos números de organización") && !one.contains("cmp-table"));
    let junk = f.get(&f.user_s, "/comparar?o=abc&o=5560125790&lang=es").await.body;
    assert!(junk.contains("No son números de organización válidos: abc") && !junk.contains("cmp-table"));
    // Un número válido que el registro no conoce sale como columna que no se pudo cargar.
    let miss = f.get(&f.user_s, "/comparar?o=5567037485&o=5560160680&lang=es").await.body;
    assert!(miss.contains("No se pudo cargar 5560160680") && miss.contains("Cykelbolaget AB"));
    // Duplicados: una sola columna → pide dos.
    assert!(f.get(&f.user_s, "/comparar?o=5567037485&o=556703-7485&lang=es").await.body.contains("Indica al menos dos"));
    // Idiomas.
    let sv = f.get(&f.user_s, "/comparar?o=5567037485&o=5560125790&lang=sv").await.body;
    assert!(sv.contains("Företagsform") && sv.contains("Omsättningstillväxt"));
    unset_bolagsverket_env();
}

#[test]
fn best_value_of_a_row_is_highlighted_only_with_two_or_more_values() {
    use crate::views_tools::best_flags;
    assert_eq!(best_flags(&[Some(1.0), Some(5.0), None, Some(5.0)]), [false, true, false, true], "los empates se marcan los dos");
    assert_eq!(best_flags(&[Some(1.0), None]), [false, false], "con un solo dato no hay con qué comparar");
    assert_eq!(best_flags(&[None, None]), [false, false]);
    assert_eq!(best_flags(&[Some(-3.0), Some(-1.0)]), [false, true], "el menos negativo es el mejor");
}

#[tokio::test]
async fn history_lists_what_each_person_viewed_and_searched() {
    let mut f = fx();
    let path = with_registry(&mut f, crate::registry::tests::SAMPLE);
    let log = |user: &User, event: &str, detail: &str| {
        f.st.db.log(&crate::db::NewActivity { user_id: Some(user.id), user_label: &user.label(), role: Some(user.role), event, method: "GET", path: "/", query: "", status: 200, ip: "", ua: "", detail });
    };
    log(&f.user, "company_view", "5560000019"); // el nombre sale del índice del registro
    log(&f.user, "company_view", "5599999993"); // sin nombre conocido: se enseña el número
    log(&f.user, "search", "volvo & co");
    log(&f.admin, "search", "secreto de otra persona");
    let r = f.get(&f.user_s, "/historial?lang=es").await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("Nordlys Logistik AB") && r.body.contains(r#"href="/foretag/5560000019""#));
    assert!(r.body.contains("559999-9993"), "sin nombre se enseña el número formateado");
    assert!(r.body.contains(r#"href="/sok?q=volvo%20%26%20co""#) && r.body.contains("volvo &amp; co"), "la búsqueda se escapa y se puede repetir");
    assert!(!r.body.contains("secreto de otra persona"), "cada persona ve solo su historial");
    // Vacío para quien no ha hecho nada.
    assert!(f.get(&f.sa_s, "/historial?lang=es").await.body.contains("Todavía no hay nada aquí"));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[tokio::test]
async fn new_screens_require_login_and_are_translated() {
    let f = fx();
    for path in ["/bevakning", "/comparar", "/historial"] {
        let r = f.get_anon(path).await;
        assert_eq!(r.status, StatusCode::SEE_OTHER, "{path}");
        assert!(r.location().unwrap().starts_with("/login?next="));
    }
    for path in ["/bevakning/add", "/bevakning/remove", "/bevakning/refresh"] {
        let r = send(&f.st, "POST", path, None, Some(form(&[("orgnr", "5299999994")])), &[]).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{path}");
    }
    for lang in Lang::ALL {
        for path in ["/bevakning", "/comparar", "/comparar?o=x&o=y", "/historial"] {
            let sep = if path.contains('?') { '&' } else { '?' };
            let r = f.get(&f.user_s, &format!("{path}{sep}lang={}", lang.code())).await;
            assert_eq!(r.status, StatusCode::OK, "{path}");
            assert!(!r.body.contains('⟦'), "{lang:?} {path}");
        }
    }
}
