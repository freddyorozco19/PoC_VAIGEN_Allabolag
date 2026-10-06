//! Siffra — réplica en Rust del scaffold Next.js (MVP de inteligencia financiera de empresas suecas).
//!
//! Servidor Axum con HTML renderizado en servidor (maud), en español, inglés y sueco. Acceso con cuentas
//! (superadmin, admin, user) guardadas en SQLite y registro de actividad que solo ve el superadmin.

mod analysis;
mod annual_report;
mod app;
mod auth;
mod bolagsverket;
mod catalog;
mod db;
mod esef;
mod format;
mod handlers;
mod handlers_data;
mod handlers_tools;
mod i18n;
mod model;
mod registry;
mod scb;
mod summary;
mod util;
mod views;
mod views_admin;
mod views_data;
mod views_fin;
mod views_tools;

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
    use handlers_data as d;
    use handlers_tools as t;
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
        .route("/datos", get(d::datos_get))
        .route("/datos.csv", get(d::datos_csv))
        // Aplicación
        .route("/sok", get(h::sok))
        .route("/foretag/:org", get(h::company))
        .route("/foretag/:org/benchmarks", get(h::company_benchmarks))
        .route("/foretag/:org/bokslut", get(h::company_bokslut))
        // Seguimiento, comparación e historial (datos reales)
        .route("/bevakning", get(t::watch_get))
        .route("/bevakning/add", post(t::watch_add_post))
        .route("/bevakning/remove", post(t::watch_remove_post))
        .route("/bevakning/refresh", post(t::watch_refresh_post))
        .route("/comparar", get(t::compare_get))
        .route("/historial", get(t::history_get))
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
  siffra-rs backup DESTINO                            Copia consistente de la base de datos
  siffra-rs registry-import ARCHIVO [--out registry.db] [--limit N]
                                                      Carga bolagsverket_bulkfil.zip (o su .txt) en un índice de nombres
  siffra-rs report-dump ORGNR [--dims] [--text]       Muestra todo lo que trae el último informe anual de una empresa
  siffra-rs esef-check ORGNR                          Cifras consolidadas ESEF de una cotizada (complemento de Bolagsverket)
  siffra-rs registry-stats [--db registry.db]         Cifras del índice (filas, activas, por tipo de identidad y forma)
  siffra-rs registry-search TEXTO [--db registry.db] [--all] [--limit N]
                                                      Busca por nombre (--all incluye las dadas de baja)";

fn registry_path(args: &[String], flag_name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(flag(args, flag_name).or_else(|| std::env::var("SIFFRA_REGISTRY").ok()).unwrap_or_else(|| "registry.db".into()))
}

/// Ejecuta un subcomando y devuelve `true` si lo era (entonces no se inicia el servidor).
async fn run_cli(args: &[String]) -> bool {
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
        "registry-import" => {
            let src = rest.first().cloned().unwrap_or_else(|| fail(USAGE));
            let out = registry_path(rest, "--out");
            let limit = flag(rest, "--limit").and_then(|l| l.parse::<u64>().ok());
            let reader = registry::open_source(std::path::Path::new(&src)).unwrap_or_else(|e| fail(&e));
            let started = std::time::Instant::now();
            let source = std::path::Path::new(&src).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(src.clone());
            let report = registry::import(reader, &out, limit, &source, 200_000, |n| {
                eprintln!("  {n} filas leídas ({:.0} s)", started.elapsed().as_secs_f64());
            })
            .unwrap_or_else(|e| fail(&format!("Importación fallida: {e}")));
            println!(
                "Importadas {} filas ({} leídas, {} sin la forma esperada) en {:.0} s → {}",
                report.inserted,
                report.read,
                report.skipped,
                started.elapsed().as_secs_f64(),
                out.display()
            );
            true
        }
        "report-dump" => {
            // Diagnóstico: todo lo que trae el último informe anual de una empresa (hechos numéricos del total
            // de la empresa; con --dims también los desgloses; con --text los textos).
            let orgnr = rest.first().cloned().unwrap_or_else(|| fail(USAGE));
            let Some(id) = bolagsverket::normalize_org_number(&orgnr) else { fail("Número de organización no válido.") };
            let docs = bolagsverket::list_documents(&id).await.unwrap_or_else(|e| fail(&e.to_string()));
            let mut docs = docs;
            docs.sort_by(|a, b| b.period_end.cmp(&a.period_end));
            let Some(doc) = docs.first() else { fail("La empresa no tiene informes anuales digitales.") };
            println!("{} informes; el más reciente: ejercicio {} (id {})", docs.len(), doc.period_end, doc.id);
            let zip = bolagsverket::download_document(&doc.id).await.unwrap_or_else(|e| fail(&e.to_string()));
            let xhtml = annual_report::extract_xhtml(&zip).unwrap_or_else(|e| fail(&e));
            let facts = annual_report::parse_all(&xhtml).unwrap_or_else(|e| fail(&e));
            let numeric = facts.iter().filter(|f| f.value.is_some()).count();
            let dimensional = facts.iter().filter(|f| !f.dims.is_empty()).count();
            println!("{} hechos: {} con cifra, {} con desglose, {} de texto ({} KB descomprimido)", facts.len(), numeric, dimensional, facts.len() - numeric, xhtml.len() / 1024);
            let want_dims = rest.iter().any(|a| a == "--dims");
            let want_text = rest.iter().any(|a| a == "--text");
            let mut shown: Vec<&annual_report::RawFact> = facts.iter().filter(|f| (f.value.is_some() && (want_dims || f.dims.is_empty())) || (want_text && f.value.is_none())).collect();
            shown.sort_by(|a, b| a.concept.cmp(&b.concept).then(a.end.cmp(&b.end)).then(a.instant.cmp(&b.instant)).then(a.dims.cmp(&b.dims)));
            for f in shown {
                let period = f.instant.clone().unwrap_or_else(|| format!("{}..{}", f.start.clone().unwrap_or_default(), f.end.clone().unwrap_or_default()));
                let value = match f.value {
                    Some(v) => format!("{v} {}", f.unit.clone().unwrap_or_default()),
                    None => f.text.clone().unwrap_or_default().chars().take(90).collect::<String>().replace('\n', " "),
                };
                println!("{:<62} {:<23} {}{}", f.concept, period, value, if f.dims.is_empty() { String::new() } else { format!("  [{}]", f.dims) });
            }
            true
        }
        "esef-check" => {
            // Diagnóstico: ¿hay informe ESEF (cotizadas) de esta empresa? Muestra las cifras de grupo por ejercicio.
            let orgnr = rest.first().cloned().unwrap_or_else(|| fail(USAGE));
            let Some(id) = bolagsverket::normalize_org_number(&orgnr) else { fail("Número de organización no válido.") };
            let reports = esef::load_reports(None, &id).await.unwrap_or_else(|e| fail(&e));
            if reports.is_empty() {
                println!("Sin informes ESEF para {id} (no es cotizada, no tiene LEI, o el informe no está en coronas).");
            } else {
                let fin = annual_report::merge_raw(&reports);
                println!("{} informes ESEF leídos; cifras consolidadas en tkr:", reports.len());
                let n = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_else(|| "-".into());
                println!("{:<6} {:>14} {:>14} {:>14} {:>14} {:>14}", "año", "facturación", "res.explot.", "res.antes imp.", "activos", "patrimonio");
                for y in &fin.years {
                    println!("{:<6} {:>14} {:>14} {:>14} {:>14} {:>14}", y.label, n(y.revenue), n(y.operating_result), n(y.result), n(y.assets), n(y.equity));
                }
            }
            true
        }
        "registry-stats" => {
            let path = registry_path(rest, "--db");
            let s = registry::Registry::open(&path).unwrap_or_else(|e| fail(&e)).stats();
            println!("Índice {} (fuente {}, importado {})", path.display(), s.source, s.imported_at);
            println!("Filas: {}   organizaciones distintas: {}   filas activas: {}", s.rows, s.distinct_orgnr, s.active_rows);
            println!("Por tipo de identidad:");
            for (k, n) in &s.by_id_type {
                println!("  {k:<14} {n}");
            }
            println!("Por forma jurídica (filas / activas):");
            for (k, n, a) in &s.by_form {
                println!("  {k:<12} {n:>9} {a:>9}");
            }
            true
        }
        "registry-search" => {
            let q = rest.first().cloned().unwrap_or_else(|| fail(USAGE));
            let reg = registry::Registry::open(&registry_path(rest, "--db")).unwrap_or_else(|e| fail(&e));
            let limit = flag(rest, "--limit").and_then(|l| l.parse().ok()).unwrap_or(15);
            let started = std::time::Instant::now();
            let hits = reg.search(&q, limit, rest.iter().any(|a| a == "--all"));
            for h in &hits {
                println!("{}  {:<44} {:<10} {:<12} {}", h.orgnr, h.name, h.form, h.city.as_deref().unwrap_or("-"), h.dereg.as_deref().map(|d| format!("baja {d}")).unwrap_or_default());
            }
            println!("{} resultados en {:.1} ms", hits.len(), started.elapsed().as_secs_f64() * 1000.0);
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
    if run_cli(&args).await {
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
    // Todos los informes que se descarguen se guardan enteros (hechos en la base de datos, documento en disco).
    annual_report::set_store(annual_report::Store { db: db.clone(), dir: std::env::var("SIFFRA_REPORTS").ok().filter(|d| !d.is_empty()).map(std::path::PathBuf::from) });
    let mut state = AppState::new(db.clone());
    state.demo = std::env::var("SIFFRA_DEMO").is_ok_and(|v| v == "1");
    state.force_secure = std::env::var("SIFFRA_SECURE_COOKIES").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));

    // Búsqueda por nombre: índice del archivo oficial de Bolagsverket. Con SIFFRA_REGISTRY_REFRESH=1 el servidor lo
    // descarga solo (y lo renueva cada semana); sin esa variable solo usa el archivo que ya exista.
    state.registry = registry::RegistryHandle::new(registry_path(&[], "--db"));
    match state.registry.get() {
        Some(r) => println!("Índice de empresas: {} filas", r.row_count()),
        None => println!("Índice de empresas: sin cargar (la búsqueda por nombre no está disponible)"),
    }
    if std::env::var("SIFFRA_REGISTRY_REFRESH").is_ok_and(|v| v == "1") {
        registry::spawn_auto_refresh(state.registry.clone());
    }

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
