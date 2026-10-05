//! Siffra — réplica en Rust del scaffold Next.js (MVP de inteligencia financiera de empresas suecas).
//!
//! Servidor Axum con HTML renderizado en servidor (maud), en español, inglés y sueco. Acceso con cuentas
//! (superadmin, admin, user) guardadas en SQLite y registro de actividad que solo ve el superadmin.

mod annual_report;
mod app;
mod auth;
mod bolagsverket;
mod catalog;
mod db;
mod format;
mod handlers;
mod i18n;
mod model;
mod scb;
mod summary;
mod util;
mod views;
mod views_admin;

#[cfg(test)]
mod tests;

use std::net::SocketAddr;

use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{middleware, Router};

use crate::app::{session_mw, AppState};
use crate::db::{Db, NewUser, Role};
use crate::i18n::Lang;

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

async fn favicon() -> StatusCode {
    StatusCode::NO_CONTENT
}

pub fn router(state: AppState) -> Router {
    use handlers as h;
    Router::new()
        .route("/", get(h::root))
        .route("/healthz", get(h::healthz))
        .route("/favicon.ico", get(favicon))
        .route("/static/styles.css", get(h::styles))
        // Cuentas
        .route("/login", get(h::login_get).post(h::login_post))
        .route("/logout", post(h::logout_post))
        .route("/profile", get(h::profile_get).post(h::profile_post))
        .route("/profile/password", post(h::profile_password_post))
        // Usuarios (admin y superadmin)
        .route("/users", get(h::users_list).post(h::user_create))
        .route("/users/new", get(h::user_new_get))
        .route("/users/:id", post(h::user_update))
        .route("/users/:id/edit", get(h::user_edit_get))
        .route("/users/:id/delete", get(h::user_delete_get).post(h::user_delete_post))
        .route("/users/:id/reset-password", post(h::user_reset_post))
        // Actividad (solo superadmin)
        .route("/activity", get(h::activity_get))
        .route("/activity.csv", get(h::activity_csv))
        // Aplicación
        .route("/sok", get(h::sok))
        .route("/foretag/:org", get(h::company))
        .route("/foretag/:org/benchmarks", get(h::company_benchmarks))
        .route("/foretag/:org/bokslut", get(h::company_bokslut))
        .route("/bevakning", get(h::bevakning))
        .route("/likviditet", get(h::likviditet))
        .route("/sie", get(h::sie))
        .route("/fakturor", get(h::fakturor))
        .fallback(h::fallback)
        .layer(middleware::from_fn_with_state(state.clone(), session_mw))
        .with_state(state)
}

// ───────────────────────── Línea de comandos ─────────────────────────

fn open_db() -> Db {
    let path = std::env::var("SIFFRA_DB").unwrap_or_else(|_| "siffra.db".to_string());
    match Db::open(&path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("No se pudo abrir la base de datos {path}: {e}");
            std::process::exit(1);
        }
    }
}

/// `--clave valor` de la lista de argumentos.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// Contraseña desde una variable de entorno (para no dejarla en el historial del shell) o generada.
fn password_for_cli(args: &[String]) -> (String, bool) {
    match flag(args, "--password-env").and_then(|var| std::env::var(var).ok()).filter(|p| !p.is_empty()) {
        Some(p) => (p, false),
        None => (util::gen_password(14), true),
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(2);
}

const USAGE: &str = "Uso:
  siffra-rs                       Inicia el servidor (PORT, SIFFRA_DB, SIFFRA_SECURE_COOKIES)
  siffra-rs user-add --role superadmin|admin|user --name NOMBRE [--email E] [--username U]
                     [--lang es|en|sv] [--password-env VAR] [--must-change]
  siffra-rs user-reset LOGIN [--password-env VAR]     Cambia la contraseña de una cuenta
  siffra-rs user-list                                 Lista las cuentas
  siffra-rs backup DESTINO                            Copia consistente de la base de datos";

/// Ejecuta un subcomando y devuelve `true` si lo era (entonces no se inicia el servidor).
fn run_cli(args: &[String]) -> bool {
    let Some(cmd) = args.first().map(String::as_str) else { return false };
    let rest = &args[1..];
    match cmd {
        "user-add" => {
            let db = open_db();
            let role = flag(rest, "--role").and_then(|r| Role::from_code(&r)).unwrap_or_else(|| fail(USAGE));
            let name = flag(rest, "--name").unwrap_or_else(|| fail(USAGE));
            let (email, username) = (flag(rest, "--email"), flag(rest, "--username"));
            if email.is_none() && username.is_none() {
                fail("Indica --email o --username.");
            }
            let lang = flag(rest, "--lang").and_then(|l| Lang::from_code(&l)).unwrap_or(Lang::DEFAULT);
            let (password, generated) = password_for_cli(rest);
            if let Some(problem) = auth::password_problem(&password) {
                fail(&format!("Contraseña no válida ({problem})."));
            }
            let new = NewUser {
                email,
                username,
                name,
                role,
                active: true,
                lang,
                pass_hash: auth::hash_password(&password),
                must_change: generated || rest.iter().any(|a| a == "--must-change"),
                created_by: None,
            };
            match db.create_user(&new) {
                Ok(id) => {
                    println!("Usuario #{id} creado ({}).", role.code());
                    if generated {
                        println!("Contraseña temporal (se pedirá cambiarla al entrar): {password}");
                    }
                }
                Err(e) => fail(&format!("No se pudo crear el usuario: {e:?}")),
            }
            true
        }
        "user-reset" => {
            let db = open_db();
            let login = rest.first().cloned().unwrap_or_else(|| fail(USAGE));
            let Some((user, _)) = db.user_by_login(&login) else { fail("No existe esa cuenta.") };
            let (password, generated) = password_for_cli(rest);
            if let Some(problem) = auth::password_problem(&password) {
                fail(&format!("Contraseña no válida ({problem})."));
            }
            if db.set_password(user.id, &auth::hash_password(&password), generated).is_err() {
                fail("No se pudo cambiar la contraseña.");
            }
            db.delete_user_sessions(user.id);
            println!("Contraseña de {} actualizada; sus sesiones se cerraron.", user.label());
            if generated {
                println!("Contraseña temporal (se pedirá cambiarla al entrar): {password}");
            }
            true
        }
        "user-list" => {
            for u in open_db().list_users("", None) {
                println!("#{:<3} {:<10} {:<8} {:<34} {}", u.id, u.role.code(), if u.active { "activo" } else { "inactivo" }, u.label(), u.name);
            }
            true
        }
        "backup" => {
            let dest = rest.first().cloned().unwrap_or_else(|| fail(USAGE));
            match open_db().backup_to(&dest) {
                Ok(()) => println!("Copia guardada en {dest}"),
                Err(e) => fail(&format!("No se pudo copiar: {e}")),
            }
            true
        }
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            true
        }
        _ => fail(USAGE),
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    load_dotenv();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if run_cli(&args) {
        return Ok(());
    }

    match bolagsverket::environment_host() {
        Some(host) if bolagsverket::configured() => println!("Bolagsverket: conectado a {host}"),
        _ => println!("Bolagsverket: sin credenciales (solo datos de EJEMPLO)"),
    }
    let db = open_db();
    if db.count_users() == 0 {
        println!("AVISO: no hay cuentas. Crea la primera con: siffra-rs user-add --role superadmin --name \"Nombre\" --username admin");
    }
    let mut state = AppState::new(db.clone());
    state.force_secure = std::env::var("SIFFRA_SECURE_COOKIES").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));

    // Limpieza periódica: sesiones caducadas y registro de actividad con más de un año.
    tokio::spawn(async move {
        loop {
            db.prune(365);
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    });

    // 3000 lo usa `npm run dev` del original; por defecto aquí 3001 para poder ejecutar ambos a la vez.
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3001);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("Siffra (Rust) escuchando en http://localhost:{port}");
    axum::serve(listener, router(state).into_make_service_with_connect_info::<SocketAddr>()).await
}
