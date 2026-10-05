//! Autenticación y permisos: hash de contraseñas (argon2id), cookies, límite de intentos y reglas por rol.
//!
//! Roles: `superadmin` (todo, incluido el registro de actividad), `admin` (gestiona usuarios de rol
//! `user`) y `user` (usa la aplicación y edita su propio perfil).

use std::collections::HashMap;
use std::sync::Mutex;

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};

use crate::db::{Role, User};
use crate::util;

pub const MIN_PASSWORD: usize = 10;
pub const SESSION_TTL_SECS: i64 = 12 * 60 * 60;
pub const SESSION_COOKIE: &str = "siffra_sid";
pub const PRE_CSRF_COOKIE: &str = "siffra_pre";
pub const LANG_COOKIE: &str = "lang";

fn hasher() -> Argon2<'static> {
    // En los tests se usan parámetros mínimos para que no tarden; en producción, los de OWASP (19 MiB, t=2, p=1).
    let params = if cfg!(test) { Params::new(8, 1, 1, None) } else { Params::new(19_456, 2, 1, None) };
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params.expect("parámetros de argon2"))
}

pub fn hash_password(password: &str) -> String {
    let salt = SaltString::encode_b64(&util::random_bytes(16)).expect("sal");
    hasher().hash_password(password.as_bytes(), &salt).expect("hash").to_string()
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|parsed| hasher().verify_password(password.as_bytes(), &parsed).is_ok())
}

/// Gasta el mismo tiempo que una verificación real, para que "usuario inexistente" no se distinga por tiempo.
pub fn dummy_verify(password: &str) {
    static DUMMY: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| hash_password("no-es-una-contraseña-real"));
    let _ = verify_password(&DUMMY, password);
}

/// Clave del catálogo con el problema de una contraseña nueva, o `None` si es aceptable.
pub fn password_problem(password: &str) -> Option<&'static str> {
    if password.chars().count() < MIN_PASSWORD {
        Some("pw.too_short")
    } else if password.chars().all(|c| c.is_ascii_digit()) || password.chars().collect::<std::collections::HashSet<_>>().len() < 4 {
        Some("pw.too_simple")
    } else {
        None
    }
}

// ───────────── Límite de intentos ─────────────

/// Máximo `MAX_FAILURES` fallos por clave en `WINDOW_SECS`. En memoria: se reinicia con el servidor.
pub struct Limiter {
    failures: Mutex<HashMap<String, Vec<i64>>>,
}

impl Limiter {
    pub const MAX_FAILURES: usize = 5;
    pub const WINDOW_SECS: i64 = 15 * 60;

    pub fn new() -> Limiter {
        Limiter { failures: Mutex::new(HashMap::new()) }
    }

    pub fn blocked(&self, key: &str) -> bool {
        let now = util::now_unix();
        let mut map = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        let list = map.entry(key.to_string()).or_default();
        list.retain(|t| now - t < Self::WINDOW_SECS);
        list.len() >= Self::MAX_FAILURES
    }

    pub fn record_failure(&self, key: &str) {
        let mut map = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(key.to_string()).or_default().push(util::now_unix());
    }

    pub fn clear(&self, key: &str) {
        self.failures.lock().unwrap_or_else(|e| e.into_inner()).remove(key);
    }
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new()
    }
}

// ───────────── Cookies ─────────────

pub fn cookie_value(cookie_header: &str, name: &str) -> Option<String> {
    cookie_header.split(';').find_map(|part| {
        let (k, v) = part.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

pub fn set_cookie(name: &str, value: &str, max_age: i64, http_only: bool, secure: bool) -> String {
    let mut c = format!("{name}={value}; Path=/; Max-Age={max_age}; SameSite=Lax");
    if http_only {
        c.push_str("; HttpOnly");
    }
    if secure {
        c.push_str("; Secure");
    }
    c
}

// ───────────── Permisos ─────────────

/// ¿Puede `actor` ver el listado de usuarios y gestionar a alguien?
pub fn can_admin_users(actor: &User) -> bool {
    actor.role >= Role::Admin
}

/// Registro de actividad: solo el superadmin.
pub fn can_view_activity(actor: &User) -> bool {
    actor.role == Role::Superadmin
}

/// ¿Puede `actor` editar a `target`? El superadmin a cualquiera; el admin solo a usuarios de rol `user`.
pub fn can_manage(actor: &User, target: &User) -> bool {
    match actor.role {
        Role::Superadmin => true,
        Role::Admin => target.role == Role::User,
        Role::User => false,
    }
}

/// Roles que `actor` puede asignar al crear o editar.
pub fn assignable_roles(actor: &User) -> Vec<Role> {
    match actor.role {
        Role::Superadmin => vec![Role::Superadmin, Role::Admin, Role::User],
        Role::Admin => vec![Role::User],
        Role::User => vec![],
    }
}

/// Borrar una cuenta: no la propia y nunca el último superadmin activo.
pub fn can_delete(actor: &User, target: &User, active_superadmins: i64) -> bool {
    can_manage(actor, target) && actor.id != target.id && !(target.role == Role::Superadmin && target.active && active_superadmins <= 1)
}

/// Cambios que dejarían la plataforma sin superadmin o bloquearían a quien edita: se rechazan.
/// Devuelve la clave del catálogo del problema.
pub fn forbidden_change(actor: &User, target: &User, new_role: Role, new_active: bool, active_superadmins: i64) -> Option<&'static str> {
    if !can_manage(actor, target) {
        return Some("err.forbidden");
    }
    if !assignable_roles(actor).contains(&new_role) {
        return Some("usr.err.role_not_allowed");
    }
    if actor.id == target.id && (new_role != target.role || !new_active) {
        return Some("usr.err.own_role_status");
    }
    let removes_a_superadmin = target.role == Role::Superadmin && target.active && (new_role != Role::Superadmin || !new_active);
    if removes_a_superadmin && active_superadmins <= 1 {
        return Some("usr.err.last_superadmin");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;

    fn user(id: i64, role: Role) -> User {
        User { id, email: Some(format!("u{id}@x.co")), username: None, name: format!("U{id}"), role, active: true, lang: Lang::Es, must_change: false, created_at: String::new(), last_login: None, created_by: None }
    }

    #[test]
    fn password_round_trip_and_salting() {
        let h = hash_password("Contraseña-segura-1");
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password(&h, "Contraseña-segura-1"));
        assert!(!verify_password(&h, "otra"));
        assert!(!verify_password("no-es-un-hash", "x"));
        assert_ne!(hash_password("Contraseña-segura-1"), h, "cada hash lleva su propia sal");
    }

    #[test]
    fn password_policy() {
        assert_eq!(password_problem("corta"), Some("pw.too_short"));
        assert_eq!(password_problem("1234567890123"), Some("pw.too_simple"));
        assert_eq!(password_problem("aaaaaaaaaaaa"), Some("pw.too_simple"));
        assert_eq!(password_problem("Una-frase-larga-2026"), None);
    }

    #[test]
    fn limiter_blocks_after_five_failures_and_resets() {
        let l = Limiter::new();
        for _ in 0..4 {
            l.record_failure("ip|ana");
        }
        assert!(!l.blocked("ip|ana"));
        l.record_failure("ip|ana");
        assert!(l.blocked("ip|ana"));
        assert!(!l.blocked("ip|otra"));
        l.clear("ip|ana");
        assert!(!l.blocked("ip|ana"));
    }

    #[test]
    fn cookies() {
        assert_eq!(cookie_value("a=1; siffra_sid=abc; lang=es", "siffra_sid").as_deref(), Some("abc"));
        assert_eq!(cookie_value("a=1", "siffra_sid"), None);
        let c = set_cookie("x", "y", 60, true, true);
        assert!(c.contains("HttpOnly") && c.contains("Secure") && c.contains("SameSite=Lax") && c.contains("Max-Age=60"));
        assert!(!set_cookie("x", "y", 60, false, false).contains("HttpOnly"));
    }

    #[test]
    fn role_permissions() {
        let (sa, ad, us) = (user(1, Role::Superadmin), user(2, Role::Admin), user(3, Role::User));
        assert!(can_admin_users(&sa) && can_admin_users(&ad) && !can_admin_users(&us));
        assert!(can_view_activity(&sa) && !can_view_activity(&ad) && !can_view_activity(&us));
        assert!(can_manage(&sa, &ad) && can_manage(&ad, &us) && !can_manage(&ad, &sa) && !can_manage(&ad, &user(4, Role::Admin)));
        assert!(!can_manage(&us, &us));
        assert_eq!(assignable_roles(&ad), vec![Role::User]);
        assert!(assignable_roles(&us).is_empty());
    }

    #[test]
    fn deletion_rules() {
        let (sa, ad, us) = (user(1, Role::Superadmin), user(2, Role::Admin), user(3, Role::User));
        assert!(can_delete(&sa, &us, 1) && can_delete(&ad, &us, 1) && can_delete(&sa, &ad, 1));
        assert!(!can_delete(&sa, &sa, 2), "no la propia cuenta");
        assert!(!can_delete(&ad, &sa, 2), "un admin no borra a un superadmin");
        assert!(!can_delete(&sa, &user(9, Role::Superadmin), 1), "nunca el último superadmin activo");
        assert!(can_delete(&sa, &user(9, Role::Superadmin), 2));
    }

    #[test]
    fn forbidden_changes() {
        let (sa, ad, us) = (user(1, Role::Superadmin), user(2, Role::Admin), user(3, Role::User));
        assert_eq!(forbidden_change(&ad, &us, Role::Admin, true, 1), Some("usr.err.role_not_allowed"), "un admin no asciende");
        assert_eq!(forbidden_change(&ad, &sa, Role::User, true, 1), Some("err.forbidden"));
        assert_eq!(forbidden_change(&sa, &sa, Role::Admin, true, 2), Some("usr.err.own_role_status"));
        assert_eq!(forbidden_change(&sa, &sa, Role::Superadmin, false, 2), Some("usr.err.own_role_status"));
        assert_eq!(forbidden_change(&sa, &user(9, Role::Superadmin), Role::Admin, true, 1), Some("usr.err.last_superadmin"));
        assert_eq!(forbidden_change(&sa, &user(9, Role::Superadmin), Role::Superadmin, false, 1), Some("usr.err.last_superadmin"));
        assert_eq!(forbidden_change(&sa, &user(9, Role::Superadmin), Role::Admin, true, 2), None);
        assert_eq!(forbidden_change(&ad, &us, Role::User, false, 1), None, "desactivar a un usuario sí puede");
        assert_eq!(forbidden_change(&us, &us, Role::User, true, 1), Some("err.forbidden"));
    }
}
