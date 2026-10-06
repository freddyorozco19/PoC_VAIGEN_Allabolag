//! Persistencia en SQLite: usuarios, sesiones y registro de actividad.
//!
//! Una única conexión protegida por `Mutex` (operaciones muy cortas; la app es de uso interno).
//! Las fechas se guardan como texto ISO-8601 en UTC, que ordena igual que el tiempo.

use std::sync::{Arc, Mutex};

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};

use crate::annual_report::RawFact;
use crate::i18n::Lang;
use crate::util;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Role {
    User = 0,
    Admin = 1,
    Superadmin = 2,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Superadmin, Role::Admin, Role::User];

    pub fn code(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Admin => "admin",
            Role::Superadmin => "superadmin",
        }
    }

    pub fn from_code(code: &str) -> Option<Role> {
        match code {
            "user" => Some(Role::User),
            "admin" => Some(Role::Admin),
            "superadmin" => Some(Role::Superadmin),
            _ => None,
        }
    }

    /// Clave del catálogo con el nombre del rol.
    pub fn label_key(self) -> &'static str {
        match self {
            Role::User => "role.user",
            Role::Admin => "role.admin",
            Role::Superadmin => "role.superadmin",
        }
    }
}

#[derive(Clone, Debug)]
pub struct User {
    pub id: i64,
    pub email: Option<String>,
    pub username: Option<String>,
    pub name: String,
    pub role: Role,
    pub active: bool,
    pub lang: Lang,
    pub must_change: bool,
    pub created_at: String,
    pub last_login: Option<String>,
    pub created_by: Option<i64>,
}

impl User {
    /// Identificador para mostrar: correo o, si no tiene, nombre de usuario.
    pub fn label(&self) -> String {
        self.email.clone().or_else(|| self.username.clone()).unwrap_or_else(|| format!("#{}", self.id))
    }

    pub fn initials(&self) -> String {
        let mut it = self.name.split_whitespace().filter_map(|w| w.chars().next());
        let first = it.next();
        let last = it.last();
        first.into_iter().chain(last).flat_map(char::to_uppercase).collect::<String>().chars().take(2).collect()
    }
}

pub struct NewUser {
    pub email: Option<String>,
    pub username: Option<String>,
    pub name: String,
    pub role: Role,
    pub active: bool,
    pub lang: Lang,
    pub pass_hash: String,
    pub must_change: bool,
    pub created_by: Option<i64>,
}

#[derive(Debug, PartialEq)]
pub enum DbError {
    /// El campo (`email` o `username`) ya existe.
    Duplicate(&'static str),
    Other(String),
}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        let text = e.to_string();
        if text.contains("UNIQUE constraint failed: users.email") {
            DbError::Duplicate("email")
        } else if text.contains("UNIQUE constraint failed: users.username") {
            DbError::Duplicate("username")
        } else {
            DbError::Other(text)
        }
    }
}

#[derive(Clone, Debug)]
pub struct ActivityRow {
    pub id: i64,
    pub ts: String,
    pub user_id: Option<i64>,
    pub user_label: String,
    pub role: Option<String>,
    pub event: String,
    pub method: String,
    pub path: String,
    pub query: String,
    pub status: i64,
    pub ip: String,
    pub ua: String,
    pub detail: String,
}

pub struct NewActivity<'a> {
    pub user_id: Option<i64>,
    pub user_label: &'a str,
    pub role: Option<Role>,
    pub event: &'a str,
    pub method: &'a str,
    pub path: &'a str,
    pub query: &'a str,
    pub status: u16,
    pub ip: &'a str,
    pub ua: &'a str,
    pub detail: &'a str,
}

#[derive(Default, Clone, Debug)]
pub struct ActivityFilter {
    pub user_id: Option<i64>,
    pub event: Option<String>,
    pub q: Option<String>,
    /// "AAAA-MM-DD" (inclusive)
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Clone, Debug)]
pub struct UserSummary {
    pub user_id: i64,
    pub name: String,
    pub label: String,
    pub role: Role,
    pub active: bool,
    pub last_login: Option<String>,
    pub last_seen: Option<String>,
    pub logins: i64,
    pub views: i64,
    pub searches: i64,
    pub companies: i64,
}

#[derive(Clone, Debug, Default)]
pub struct Kpis {
    pub events_24h: i64,
    pub active_users_24h: i64,
    pub searches_24h: i64,
    pub failed_logins_24h: i64,
    pub total_users: i64,
}

/// Un informe anual guardado.
#[derive(Clone, Debug, PartialEq)]
pub struct ReportInfo {
    pub doc_id: String,
    pub orgnr: String,
    pub period_end: String,
    pub registered: String,
    pub fetched_at: String,
    pub fact_count: i64,
    /// Ruta del documento original (ZIP) guardado en disco, si se guardó.
    pub raw_path: Option<String>,
}

/// Máximo de empresas que una persona puede seguir.
pub const WATCH_LIMIT: i64 = 50;

/// Una empresa seguida por una persona, con la última foto que se tomó de ella.
#[derive(Clone, Debug, PartialEq)]
pub struct WatchRow {
    pub orgnr: String,
    pub name: String,
    pub form: String,
    pub added_at: String,
    /// `None` = aún sin revisar; `Some("")` = sin valorar; `good` / `warn` / `bad`.
    pub level: Option<String>,
    pub year: Option<String>,
    pub revenue: Option<i64>,
    pub result: Option<i64>,
    pub solidity: Option<f64>,
    pub flags: i64,
    pub checked_at: Option<String>,
    /// `level` (de `change_from` a `change_to`, niveles) o `status` (marcas del registro).
    pub change_kind: Option<String>,
    pub change_from: Option<String>,
    pub change_to: Option<String>,
    pub changed_at: Option<String>,
}

/// Lo que se guarda de una revisión de la empresa. `level`: `""` (sin valorar), `good`, `warn` o `bad`.
#[derive(Clone, Debug)]
pub struct WatchSnapshot {
    pub name: String,
    pub form: String,
    pub level: String,
    pub year: Option<String>,
    pub revenue: Option<i64>,
    pub result: Option<i64>,
    pub solidity: Option<f64>,
    pub flags: i64,
}

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS users (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    email       TEXT COLLATE NOCASE UNIQUE,
    username    TEXT COLLATE NOCASE UNIQUE,
    name        TEXT NOT NULL,
    role        TEXT NOT NULL CHECK (role IN ('superadmin','admin','user')),
    active      INTEGER NOT NULL DEFAULT 1,
    lang        TEXT NOT NULL DEFAULT 'es',
    pass_hash   TEXT NOT NULL,
    must_change INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    last_login  TEXT,
    created_by  INTEGER,
    CHECK (email IS NOT NULL OR username IS NOT NULL)
);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL,
    csrf       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    ip         TEXT,
    ua         TEXT
);
CREATE INDEX IF NOT EXISTS sessions_user ON sessions(user_id);
CREATE TABLE IF NOT EXISTS activity (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    ts         TEXT NOT NULL,
    user_id    INTEGER,
    user_label TEXT NOT NULL,
    role       TEXT,
    event      TEXT NOT NULL,
    method     TEXT,
    path       TEXT,
    query      TEXT,
    status     INTEGER,
    ip         TEXT,
    ua         TEXT,
    detail     TEXT
);
CREATE INDEX IF NOT EXISTS activity_ts ON activity(ts);
CREATE INDEX IF NOT EXISTS activity_user ON activity(user_id, ts);
CREATE INDEX IF NOT EXISTS activity_event ON activity(event, ts);
CREATE TABLE IF NOT EXISTS watchlist (
    user_id     INTEGER NOT NULL,
    orgnr       TEXT NOT NULL,
    name        TEXT NOT NULL,
    form        TEXT NOT NULL DEFAULT '',
    added_at    TEXT NOT NULL,
    level       TEXT,
    year        TEXT,
    revenue     INTEGER,
    result      INTEGER,
    solidity    REAL,
    flags       INTEGER NOT NULL DEFAULT 0,
    checked_at  TEXT,
    change_kind TEXT,
    change_from TEXT,
    change_to   TEXT,
    changed_at  TEXT,
    PRIMARY KEY (user_id, orgnr)
);
CREATE INDEX IF NOT EXISTS watchlist_org ON watchlist(orgnr);
CREATE TABLE IF NOT EXISTS report (
    doc_id      TEXT PRIMARY KEY,
    orgnr       TEXT NOT NULL,
    period_end  TEXT NOT NULL,
    registered  TEXT NOT NULL DEFAULT '',
    fetched_at  TEXT NOT NULL,
    fact_count  INTEGER NOT NULL,
    raw_path    TEXT
);
CREATE INDEX IF NOT EXISTS report_org ON report(orgnr, period_end);
CREATE TABLE IF NOT EXISTS fact (
    doc_id  TEXT NOT NULL,
    ctx     TEXT NOT NULL DEFAULT '',
    concept TEXT NOT NULL,
    value   REAL,
    text    TEXT,
    unit    TEXT,
    scale   INTEGER NOT NULL DEFAULT 0,
    instant TEXT,
    start   TEXT,
    end     TEXT,
    dims    TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS fact_doc ON fact(doc_id);
";

const USER_COLS: &str = "id, email, username, name, role, active, lang, must_change, created_at, last_login, created_by";

fn user_from_row(r: &Row) -> rusqlite::Result<User> {
    Ok(User {
        id: r.get(0)?,
        email: r.get(1)?,
        username: r.get(2)?,
        name: r.get(3)?,
        role: Role::from_code(&r.get::<_, String>(4)?).unwrap_or(Role::User),
        active: r.get::<_, i64>(5)? != 0,
        lang: Lang::from_code(&r.get::<_, String>(6)?).unwrap_or(Lang::DEFAULT),
        must_change: r.get::<_, i64>(7)? != 0,
        created_at: r.get(8)?,
        last_login: r.get(9)?,
        created_by: r.get(10)?,
    })
}

fn activity_from_row(r: &Row) -> rusqlite::Result<ActivityRow> {
    let s = |i: usize| r.get::<_, Option<String>>(i).map(Option::unwrap_or_default);
    Ok(ActivityRow {
        id: r.get(0)?,
        ts: r.get(1)?,
        user_id: r.get(2)?,
        user_label: r.get(3)?,
        role: r.get(4)?,
        event: r.get(5)?,
        method: s(6)?,
        path: s(7)?,
        query: s(8)?,
        status: r.get::<_, Option<i64>>(9)?.unwrap_or(0),
        ip: s(10)?,
        ua: s(11)?,
        detail: s(12)?,
    })
}

/// WHERE dinámico del registro de actividad (todo parametrizado).
fn activity_where(f: &ActivityFilter) -> (String, Vec<String>) {
    let mut clauses: Vec<String> = Vec::new();
    let mut args: Vec<String> = Vec::new();
    if let Some(id) = f.user_id {
        clauses.push("user_id = ?".into());
        args.push(id.to_string());
    }
    if let Some(e) = f.event.as_ref().filter(|e| !e.is_empty()) {
        clauses.push("event = ?".into());
        args.push(e.clone());
    }
    if let Some(q) = f.q.as_ref().filter(|q| !q.trim().is_empty()) {
        clauses.push("(path LIKE ? ESCAPE '\\' OR detail LIKE ? ESCAPE '\\' OR user_label LIKE ? ESCAPE '\\')".into());
        let like = format!("%{}%", q.trim().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        args.extend([like.clone(), like.clone(), like]);
    }
    if let Some(from) = f.from.as_ref().filter(|d| !d.is_empty()) {
        clauses.push("ts >= ?".into());
        args.push(format!("{from}T00:00:00Z"));
    }
    if let Some(to) = f.to.as_ref().filter(|d| !d.is_empty()) {
        clauses.push("ts <= ?".into());
        args.push(format!("{to}T23:59:59Z"));
    }
    let sql = if clauses.is_empty() { String::new() } else { format!(" WHERE {}", clauses.join(" AND ")) };
    (sql, args)
}

impl Db {
    pub fn open(path: &str) -> Result<Db, String> {
        if let Some(dir) = std::path::Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;
        }
        let conn = Connection::open(path).map_err(|e| format!("no se pudo abrir la base de datos {path}: {e}"))?;
        Db::init(conn)
    }

    pub fn memory() -> Db {
        Db::init(Connection::open_in_memory().expect("sqlite en memoria")).expect("esquema")
    }

    fn init(conn: Connection) -> Result<Db, String> {
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")
            .map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(Db { conn: Arc::new(Mutex::new(conn)) })
    }

    fn c(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Copia consistente de la base de datos a `path` (`VACUUM INTO`); falla si el destino ya existe.
    pub fn backup_to(&self, path: &str) -> Result<(), String> {
        self.c().execute("VACUUM INTO ?1", [path]).map(|_| ()).map_err(|e| e.to_string())
    }

    // ───────────── usuarios ─────────────

    pub fn user_by_id(&self, id: i64) -> Option<User> {
        self.c().query_row(&format!("SELECT {USER_COLS} FROM users WHERE id = ?1"), [id], user_from_row).optional().ok().flatten()
    }

    /// Usuario y hash por correo o nombre de usuario (sin distinguir mayúsculas).
    pub fn user_by_login(&self, ident: &str) -> Option<(User, String)> {
        let ident = ident.trim();
        self.c()
            .query_row(
                &format!("SELECT {USER_COLS}, pass_hash FROM users WHERE email = ?1 OR username = ?1"),
                [ident],
                |r| Ok((user_from_row(r)?, r.get::<_, String>(11)?)),
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn hash_of(&self, id: i64) -> Option<String> {
        self.c().query_row("SELECT pass_hash FROM users WHERE id = ?1", [id], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn list_users(&self, q: &str, role: Option<Role>) -> Vec<User> {
        let like = format!("%{}%", q.trim().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        let mut sql = format!(
            "SELECT {USER_COLS} FROM users WHERE (name LIKE ?1 ESCAPE '\\' OR IFNULL(email,'') LIKE ?1 ESCAPE '\\' OR IFNULL(username,'') LIKE ?1 ESCAPE '\\')"
        );
        let mut args = vec![like];
        if let Some(r) = role {
            sql.push_str(" AND role = ?2");
            args.push(r.code().to_string());
        }
        sql.push_str(" ORDER BY CASE role WHEN 'superadmin' THEN 0 WHEN 'admin' THEN 1 ELSE 2 END, name COLLATE NOCASE");
        let conn = self.c();
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params_from_iter(args.iter()), user_from_row).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    pub fn count_users(&self) -> i64 {
        self.c().query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0)).unwrap_or(0)
    }

    pub fn count_active_superadmins(&self) -> i64 {
        self.c().query_row("SELECT COUNT(*) FROM users WHERE role='superadmin' AND active=1", [], |r| r.get(0)).unwrap_or(0)
    }

    pub fn create_user(&self, u: &NewUser) -> Result<i64, DbError> {
        let now = util::now_iso();
        let conn = self.c();
        conn.execute(
            "INSERT INTO users (email, username, name, role, active, lang, pass_hash, must_change, created_at, updated_at, created_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10)",
            params![
                u.email, u.username, u.name, u.role.code(), u.active as i64, u.lang.code(), u.pass_hash, u.must_change as i64, now, u.created_by
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_user(&self, id: i64, name: &str, email: Option<&str>, username: Option<&str>, role: Role, active: bool, lang: Lang) -> Result<(), DbError> {
        self.c().execute(
            "UPDATE users SET name=?2, email=?3, username=?4, role=?5, active=?6, lang=?7, updated_at=?8 WHERE id=?1",
            params![id, name, email, username, role.code(), active as i64, lang.code(), util::now_iso()],
        )?;
        Ok(())
    }

    pub fn update_profile(&self, id: i64, name: &str, lang: Lang) -> Result<(), DbError> {
        self.c().execute("UPDATE users SET name=?2, lang=?3, updated_at=?4 WHERE id=?1", params![id, name, lang.code(), util::now_iso()])?;
        Ok(())
    }

    pub fn set_lang(&self, id: i64, lang: Lang) {
        let _ = self.c().execute("UPDATE users SET lang=?2 WHERE id=?1", params![id, lang.code()]);
    }

    pub fn set_password(&self, id: i64, hash: &str, must_change: bool) -> Result<(), DbError> {
        self.c().execute("UPDATE users SET pass_hash=?2, must_change=?3, updated_at=?4 WHERE id=?1", params![id, hash, must_change as i64, util::now_iso()])?;
        Ok(())
    }

    pub fn touch_login(&self, id: i64) {
        let _ = self.c().execute("UPDATE users SET last_login=?2 WHERE id=?1", params![id, util::now_iso()]);
    }

    /// Borra la cuenta y sus sesiones. El historial de actividad se conserva (con la etiqueta de la cuenta).
    pub fn delete_user(&self, id: i64) -> Result<bool, DbError> {
        let conn = self.c();
        conn.execute("DELETE FROM sessions WHERE user_id=?1", [id])?;
        Ok(conn.execute("DELETE FROM users WHERE id=?1", [id])? > 0)
    }

    // ───────────── sesiones ─────────────

    /// Crea una sesión y devuelve el token en claro (solo se guarda su hash).
    pub fn create_session(&self, user_id: i64, ttl_secs: i64, ip: &str, ua: &str) -> (String, String) {
        let token = util::random_hex(32);
        let csrf = util::random_hex(16);
        let now = util::now_unix();
        let _ = self.c().execute(
            "INSERT INTO sessions (token_hash, user_id, csrf, created_at, expires_at, ip, ua) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![util::sha256_hex(&token), user_id, csrf, now, now + ttl_secs, ip, ua],
        );
        (token, csrf)
    }

    /// Usuario activo y CSRF de una sesión vigente.
    pub fn session_user(&self, token: &str) -> Option<(User, String)> {
        let hash = util::sha256_hex(token);
        let now = util::now_unix();
        let conn = self.c();
        let (user_id, csrf): (i64, String) = conn
            .query_row("SELECT user_id, csrf FROM sessions WHERE token_hash=?1 AND expires_at>?2", params![hash, now], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .ok()??;
        let user = conn.query_row(&format!("SELECT {USER_COLS} FROM users WHERE id=?1 AND active=1"), [user_id], user_from_row).optional().ok()??;
        Some((user, csrf))
    }

    pub fn delete_session(&self, token: &str) {
        let _ = self.c().execute("DELETE FROM sessions WHERE token_hash=?1", [util::sha256_hex(token)]);
    }

    pub fn delete_user_sessions(&self, user_id: i64) {
        let _ = self.c().execute("DELETE FROM sessions WHERE user_id=?1", [user_id]);
    }

    pub fn prune(&self, activity_retention_days: i64) {
        let conn = self.c();
        let _ = conn.execute("DELETE FROM sessions WHERE expires_at <= ?1", [util::now_unix()]);
        if activity_retention_days > 0 {
            let cutoff = util::iso_from_unix(util::now_unix() - activity_retention_days * 86_400);
            let _ = conn.execute("DELETE FROM activity WHERE ts < ?1", [cutoff]);
        }
    }

    // ───────────── Informes anuales (todos los hechos) ─────────────

    /// Guarda un informe y TODOS sus hechos (reemplaza lo anterior del mismo documento).
    pub fn report_save(&self, orgnr: &str, doc_id: &str, period_end: &str, registered: &str, raw_path: Option<&str>, facts: &[RawFact]) {
        let mut conn = self.c();
        let Ok(tx) = conn.transaction() else { return };
        let _ = tx.execute("DELETE FROM fact WHERE doc_id = ?1", [doc_id]);
        let _ = tx.execute("DELETE FROM report WHERE doc_id = ?1", [doc_id]);
        {
            let Ok(mut st) = tx.prepare("INSERT INTO fact (doc_id, ctx, concept, value, text, unit, scale, instant, start, end, dims) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)") else { return };
            for f in facts {
                let _ = st.execute(params![doc_id, f.ctx, f.concept, f.value, f.text, f.unit, f.scale, f.instant, f.start, f.end, f.dims]);
            }
        }
        let _ = tx.execute(
            "INSERT INTO report (doc_id, orgnr, period_end, registered, fetched_at, fact_count, raw_path) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![doc_id, orgnr, period_end, registered, util::now_iso(), facts.len() as i64, raw_path],
        );
        let _ = tx.commit();
    }

    /// Hechos guardados de un informe, o `None` si ese documento no se ha guardado nunca.
    pub fn report_facts(&self, doc_id: &str) -> Option<Vec<RawFact>> {
        let conn = self.c();
        conn.query_row("SELECT 1 FROM report WHERE doc_id = ?1", [doc_id], |_| Ok(())).optional().ok().flatten()?;
        let mut st = conn.prepare("SELECT ctx, concept, value, text, unit, scale, instant, start, end, dims FROM fact WHERE doc_id = ?1 ORDER BY rowid").ok()?;
        let rows = st
            .query_map([doc_id], |r| {
                Ok(RawFact {
                    ctx: r.get(0)?,
                    concept: r.get(1)?,
                    value: r.get(2)?,
                    text: r.get(3)?,
                    unit: r.get(4)?,
                    scale: r.get(5)?,
                    instant: r.get(6)?,
                    start: r.get(7)?,
                    end: r.get(8)?,
                    dims: r.get(9)?,
                })
            })
            .ok()?;
        Some(rows.flatten().collect())
    }

    /// Informes guardados de una empresa, el de ejercicio más reciente primero.
    pub fn reports_of(&self, orgnr: &str) -> Vec<ReportInfo> {
        self.c()
            .prepare("SELECT doc_id, orgnr, period_end, registered, fetched_at, fact_count, raw_path FROM report WHERE orgnr = ?1 ORDER BY period_end DESC")
            .and_then(|mut s| {
                s.query_map([orgnr], |r| {
                    Ok(ReportInfo { doc_id: r.get(0)?, orgnr: r.get(1)?, period_end: r.get(2)?, registered: r.get(3)?, fetched_at: r.get(4)?, fact_count: r.get(5)?, raw_path: r.get(6)? })
                })
                .map(|it| it.flatten().collect())
            })
            .unwrap_or_default()
    }

    // ───────────── Mis empresas ─────────────

    pub fn watch_list(&self, user_id: i64) -> Vec<WatchRow> {
        let conn = self.c();
        conn.prepare(
            "SELECT orgnr, name, form, added_at, level, year, revenue, result, solidity, flags, checked_at, change_kind, change_from, change_to, changed_at
             FROM watchlist WHERE user_id = ?1 ORDER BY name COLLATE NOCASE",
        )
        .and_then(|mut s| {
            s.query_map([user_id], |r| {
                Ok(WatchRow {
                    orgnr: r.get(0)?,
                    name: r.get(1)?,
                    form: r.get(2)?,
                    added_at: r.get(3)?,
                    level: r.get(4)?,
                    year: r.get(5)?,
                    revenue: r.get(6)?,
                    result: r.get(7)?,
                    solidity: r.get(8)?,
                    flags: r.get(9)?,
                    checked_at: r.get(10)?,
                    change_kind: r.get(11)?,
                    change_from: r.get(12)?,
                    change_to: r.get(13)?,
                    changed_at: r.get(14)?,
                })
            })
            .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default()
    }

    pub fn watch_has(&self, user_id: i64, orgnr: &str) -> bool {
        self.c().query_row("SELECT 1 FROM watchlist WHERE user_id=?1 AND orgnr=?2", params![user_id, orgnr], |_| Ok(())).optional().ok().flatten().is_some()
    }

    pub fn watch_count(&self, user_id: i64) -> i64 {
        self.c().query_row("SELECT COUNT(*) FROM watchlist WHERE user_id=?1", [user_id], |r| r.get(0)).unwrap_or(0)
    }

    /// Empieza a seguir una empresa. `false` si ya la seguía.
    pub fn watch_add(&self, user_id: i64, orgnr: &str, name: &str, form: &str) -> bool {
        self.c()
            .execute(
                "INSERT OR IGNORE INTO watchlist (user_id, orgnr, name, form, added_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![user_id, orgnr, name, form, util::now_iso()],
            )
            .map(|n| n > 0)
            .unwrap_or(false)
    }

    pub fn watch_remove(&self, user_id: i64, orgnr: &str) -> bool {
        self.c().execute("DELETE FROM watchlist WHERE user_id=?1 AND orgnr=?2", params![user_id, orgnr]).map(|n| n > 0).unwrap_or(false)
    }

    /// Guarda una revisión de la empresa en la fila de TODAS las personas que la siguen y anota el cambio si el
    /// nivel de riesgo o el estado del registro son distintos de la revisión anterior. Devuelve cuántas filas tocó.
    pub fn watch_apply(&self, orgnr: &str, s: &WatchSnapshot) -> usize {
        let mut conn = self.c();
        let Ok(tx) = conn.transaction() else { return 0 };
        let now = util::now_iso();
        let rows: Vec<(i64, Option<String>, i64, Option<String>)> = tx
            .prepare("SELECT user_id, level, flags, checked_at FROM watchlist WHERE orgnr = ?1")
            .and_then(|mut st| st.query_map([orgnr], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map(|it| it.flatten().collect()))
            .unwrap_or_default();
        for (user_id, old_level, old_flags, checked) in &rows {
            // Sin revisión anterior no hay con qué comparar: la primera foto no cuenta como cambio.
            let change: Option<(&str, String, String)> = match (checked, old_level) {
                (Some(_), Some(old)) if *old != s.level => Some(("level", old.clone(), s.level.clone())),
                (Some(_), _) if *old_flags != s.flags => Some(("status", old_flags.to_string(), s.flags.to_string())),
                _ => None,
            };
            let _ = tx.execute(
                "UPDATE watchlist SET name=?3, form=?4, level=?5, year=?6, revenue=?7, result=?8, solidity=?9, flags=?10, checked_at=?11 WHERE user_id=?1 AND orgnr=?2",
                params![user_id, orgnr, s.name, s.form, s.level, s.year, s.revenue, s.result, s.solidity, s.flags, now],
            );
            if let Some((kind, from, to)) = change {
                let _ = tx.execute(
                    "UPDATE watchlist SET change_kind=?3, change_from=?4, change_to=?5, changed_at=?6 WHERE user_id=?1 AND orgnr=?2",
                    params![user_id, orgnr, kind, from, to, now],
                );
            }
        }
        let _ = tx.commit();
        rows.len()
    }

    // ───────────── Historial ─────────────

    /// Empresas que consultó una persona, la más reciente primero: (número de organización, última vez).
    pub fn recent_companies(&self, user_id: i64, limit: i64) -> Vec<(String, String)> {
        self.recent_details(user_id, "company_view", limit)
    }

    /// Búsquedas recientes de una persona (texto, última vez).
    pub fn recent_searches(&self, user_id: i64, limit: i64) -> Vec<(String, String)> {
        self.recent_details(user_id, "search", limit)
    }

    fn recent_details(&self, user_id: i64, event: &str, limit: i64) -> Vec<(String, String)> {
        self.c()
            .prepare(
                "SELECT detail, MAX(ts) AS last FROM activity
                 WHERE user_id = ?1 AND event = ?2 AND detail <> '' GROUP BY detail ORDER BY last DESC LIMIT ?3",
            )
            .and_then(|mut s| s.query_map(params![user_id, event, limit], |r| Ok((r.get(0)?, r.get(1)?))).map(|it| it.flatten().collect()))
            .unwrap_or_default()
    }

    // ───────────── actividad ─────────────

    pub fn log(&self, a: &NewActivity) {
        let _ = self.c().execute(
            "INSERT INTO activity (ts, user_id, user_label, role, event, method, path, query, status, ip, ua, detail)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                util::now_iso(),
                a.user_id,
                a.user_label,
                a.role.map(|r| r.code()),
                a.event,
                a.method,
                a.path,
                a.query,
                a.status as i64,
                a.ip,
                // User-Agent acotado: es dato de terceros y podría ser enorme.
                a.ua.chars().take(300).collect::<String>(),
                a.detail.chars().take(500).collect::<String>()
            ],
        );
    }

    pub fn activity(&self, f: &ActivityFilter, limit: i64, offset: i64) -> (Vec<ActivityRow>, i64) {
        let (wh, args) = activity_where(f);
        let conn = self.c();
        let total: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM activity{wh}"), params_from_iter(args.iter()), |r| r.get(0)).unwrap_or(0);
        let sql = format!(
            "SELECT id, ts, user_id, user_label, role, event, method, path, query, status, ip, ua, detail FROM activity{wh} ORDER BY id DESC LIMIT {limit} OFFSET {offset}"
        );
        let rows = conn
            .prepare(&sql)
            .and_then(|mut s| s.query_map(params_from_iter(args.iter()), activity_from_row).map(|r| r.flatten().collect()))
            .unwrap_or_default();
        (rows, total)
    }

    pub fn distinct_events(&self) -> Vec<String> {
        let conn = self.c();
        conn.prepare("SELECT DISTINCT event FROM activity ORDER BY event")
            .and_then(|mut s| s.query_map([], |r| r.get::<_, String>(0)).map(|r| r.flatten().collect()))
            .unwrap_or_default()
    }

    pub fn kpis(&self) -> Kpis {
        let since = util::iso_from_unix(util::now_unix() - 86_400);
        let conn = self.c();
        let one = |sql: &str| -> i64 { conn.query_row(sql, [&since], |r| r.get(0)).unwrap_or(0) };
        Kpis {
            events_24h: one("SELECT COUNT(*) FROM activity WHERE ts >= ?1"),
            active_users_24h: one("SELECT COUNT(DISTINCT user_id) FROM activity WHERE ts >= ?1 AND user_id IS NOT NULL"),
            searches_24h: one("SELECT COUNT(*) FROM activity WHERE ts >= ?1 AND event = 'search'"),
            failed_logins_24h: one("SELECT COUNT(*) FROM activity WHERE ts >= ?1 AND event = 'login_fail'"),
            total_users: conn.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0)).unwrap_or(0),
        }
    }

    /// Resumen por usuario (cada cuenta existente, aunque aún no tenga actividad).
    pub fn user_summaries(&self) -> Vec<UserSummary> {
        let conn = self.c();
        let sql = "
            SELECT u.id, u.name, IFNULL(u.email, u.username), u.role, u.active, u.last_login,
                   (SELECT MAX(ts) FROM activity a WHERE a.user_id = u.id),
                   (SELECT COUNT(*) FROM activity a WHERE a.user_id = u.id AND a.event = 'login_ok'),
                   (SELECT COUNT(*) FROM activity a WHERE a.user_id = u.id AND a.event IN ('view','search','company_view')),
                   (SELECT COUNT(*) FROM activity a WHERE a.user_id = u.id AND a.event = 'search'),
                   (SELECT COUNT(DISTINCT a.detail) FROM activity a WHERE a.user_id = u.id AND a.event = 'company_view')
            FROM users u
            ORDER BY CASE u.role WHEN 'superadmin' THEN 0 WHEN 'admin' THEN 1 ELSE 2 END, u.name COLLATE NOCASE";
        conn.prepare(sql)
            .and_then(|mut s| {
                s.query_map([], |r| {
                    Ok(UserSummary {
                        user_id: r.get(0)?,
                        name: r.get(1)?,
                        label: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        role: Role::from_code(&r.get::<_, String>(3)?).unwrap_or(Role::User),
                        active: r.get::<_, i64>(4)? != 0,
                        last_login: r.get(5)?,
                        last_seen: r.get(6)?,
                        logins: r.get(7)?,
                        views: r.get(8)?,
                        searches: r.get(9)?,
                        companies: r.get(10)?,
                    })
                })
                .map(|r| r.flatten().collect())
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_user(email: &str, role: Role) -> NewUser {
        NewUser {
            email: Some(email.into()),
            username: None,
            name: "Ana Pérez".into(),
            role,
            active: true,
            lang: Lang::Es,
            pass_hash: "hash".into(),
            must_change: false,
            created_by: None,
        }
    }

    #[test]
    fn users_crud_and_uniqueness() {
        let db = Db::memory();
        let id = db.create_user(&new_user("ana@x.co", Role::User)).unwrap();
        assert_eq!(db.user_by_id(id).unwrap().role, Role::User);
        assert_eq!(db.create_user(&new_user("ANA@x.co", Role::Admin)), Err(DbError::Duplicate("email")), "sin distinguir mayúsculas");
        db.update_user(id, "Ana P.", Some("ana@x.co"), None, Role::Admin, false, Lang::Sv).unwrap();
        let u = db.user_by_id(id).unwrap();
        assert_eq!((u.name.as_str(), u.role, u.active, u.lang), ("Ana P.", Role::Admin, false, Lang::Sv));
        assert!(db.delete_user(id).unwrap());
        assert!(db.user_by_id(id).is_none());
        assert!(!db.delete_user(id).unwrap());
    }

    #[test]
    fn login_by_email_or_username_case_insensitive() {
        let db = Db::memory();
        db.create_user(&NewUser { email: None, username: Some("architechia".into()), ..new_user("x@x.co", Role::Superadmin) }).unwrap();
        assert!(db.user_by_login("Architechia").is_some());
        assert!(db.user_by_login("nadie").is_none());
        let id = db.create_user(&new_user("Ana@X.co", Role::User)).unwrap();
        assert_eq!(db.user_by_login("ana@x.CO").unwrap().0.id, id);
    }

    #[test]
    fn sessions_expire_and_disabled_users_cannot_use_them() {
        let db = Db::memory();
        let id = db.create_user(&new_user("ana@x.co", Role::User)).unwrap();
        let (tok, csrf) = db.create_session(id, 3600, "1.2.3.4", "ua");
        let (u, c) = db.session_user(&tok).unwrap();
        assert_eq!((u.id, c), (id, csrf));
        assert!(db.session_user("otro").is_none());
        let (expired, _) = db.create_session(id, -10, "", "");
        assert!(db.session_user(&expired).is_none());
        db.update_user(id, "Ana", Some("ana@x.co"), None, Role::User, false, Lang::Es).unwrap();
        assert!(db.session_user(&tok).is_none(), "usuario desactivado");
        db.update_user(id, "Ana", Some("ana@x.co"), None, Role::User, true, Lang::Es).unwrap();
        db.delete_user_sessions(id);
        assert!(db.session_user(&tok).is_none());
    }

    #[test]
    fn activity_filters_summaries_and_kpis() {
        let db = Db::memory();
        let id = db.create_user(&new_user("ana@x.co", Role::User)).unwrap();
        let log = |event: &str, path: &str, detail: &str, user: Option<i64>| {
            db.log(&NewActivity { user_id: user, user_label: "ana@x.co", role: Some(Role::User), event, method: "GET", path, query: "", status: 200, ip: "1.1.1.1", ua: "ua", detail });
        };
        log("login_ok", "/login", "", Some(id));
        log("search", "/sok", "göteborg 100%", Some(id));
        log("company_view", "/foretag/559012-3456", "5590123456", Some(id));
        log("company_view", "/foretag/559012-3456", "5590123456", Some(id));
        log("login_fail", "/login", "mala@x.co", None);
        let (rows, total) = db.activity(&ActivityFilter::default(), 10, 0);
        assert_eq!((rows.len(), total), (5, 5));
        assert_eq!(rows[0].event, "login_fail", "las más recientes primero");
        let by_user = ActivityFilter { user_id: Some(id), ..Default::default() };
        assert_eq!(db.activity(&by_user, 10, 0).1, 4);
        let by_event = ActivityFilter { event: Some("search".into()), ..Default::default() };
        assert_eq!(db.activity(&by_event, 10, 0).0[0].detail, "göteborg 100%");
        let by_text = ActivityFilter { q: Some("100%".into()), ..Default::default() };
        assert_eq!(db.activity(&by_text, 10, 0).1, 1, "el % del texto se trata como carácter, no como comodín");
        assert_eq!(db.activity(&ActivityFilter { from: Some("2999-01-01".into()), ..Default::default() }, 10, 0).1, 0);
        let s = &db.user_summaries()[0];
        assert_eq!((s.logins, s.views, s.searches, s.companies), (1, 3, 1, 1), "empresas = distintas consultadas");
        let k = db.kpis();
        assert_eq!((k.events_24h, k.active_users_24h, k.searches_24h, k.failed_logins_24h, k.total_users), (5, 1, 1, 1, 1));
        assert_eq!(db.distinct_events(), ["company_view", "login_fail", "login_ok", "search"]);
    }

    #[test]
    fn history_survives_deleting_the_account() {
        let db = Db::memory();
        let id = db.create_user(&new_user("ana@x.co", Role::User)).unwrap();
        db.log(&NewActivity { user_id: Some(id), user_label: "ana@x.co", role: Some(Role::User), event: "view", method: "GET", path: "/sok", query: "", status: 200, ip: "", ua: "", detail: "" });
        db.delete_user(id).unwrap();
        let (rows, _) = db.activity(&ActivityFilter::default(), 10, 0);
        assert_eq!(rows[0].user_label, "ana@x.co");
    }

    fn snap(level: &str, flags: i64) -> WatchSnapshot {
        WatchSnapshot { name: "Prueba AB".into(), form: "AB-ORGFO".into(), level: level.into(), year: Some("2025".into()), revenue: Some(2_519), result: Some(176), solidity: Some(27.9), flags }
    }

    #[test]
    fn watchlist_follow_unfollow_and_isolation_between_users() {
        let db = Db::memory();
        let (ana, luis) = (db.create_user(&new_user("ana@x.co", Role::User)).unwrap(), db.create_user(&new_user("luis@x.co", Role::User)).unwrap());
        assert!(db.watch_add(ana, "5569705329", "Agartz AB", "AB-ORGFO"));
        assert!(!db.watch_add(ana, "5569705329", "Agartz AB", "AB-ORGFO"), "seguirla dos veces no la duplica");
        assert!(db.watch_has(ana, "5569705329") && !db.watch_has(luis, "5569705329"));
        db.watch_add(ana, "5560125790", "AB Volvo", "AB-ORGFO");
        let names: Vec<String> = db.watch_list(ana).into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["AB Volvo", "Agartz AB"], "ordenadas por nombre, sin distinguir mayúsculas");
        assert!(db.watch_list(luis).is_empty(), "cada persona ve solo las suyas");
        assert_eq!(db.watch_count(ana), 2);
        assert!(db.watch_remove(ana, "5560125790") && !db.watch_remove(ana, "5560125790"));
        assert_eq!(db.watch_count(ana), 1);
    }

    #[test]
    fn watchlist_detects_changes_between_reviews_but_not_the_first_one() {
        let db = Db::memory();
        let (ana, luis) = (db.create_user(&new_user("ana@x.co", Role::User)).unwrap(), db.create_user(&new_user("luis@x.co", Role::User)).unwrap());
        db.watch_add(ana, "5569705329", "Agartz AB", "AB-ORGFO");
        let fresh = &db.watch_list(ana)[0];
        assert_eq!((fresh.level.clone(), fresh.checked_at.clone()), (None, None), "aún sin revisar");

        assert_eq!(db.watch_apply("5569705329", &snap("good", 0)), 1);
        let first = &db.watch_list(ana)[0];
        assert_eq!((first.level.as_deref(), first.year.as_deref(), first.revenue), (Some("good"), Some("2025"), Some(2_519)));
        assert_eq!(first.change_kind, None, "la primera foto no es un cambio");

        // Mismo estado: nada nuevo. Luis empieza a seguirla después y no hereda cambios.
        db.watch_apply("5569705329", &snap("good", 0));
        assert_eq!(db.watch_list(ana)[0].change_kind, None);
        db.watch_add(luis, "5569705329", "Agartz AB", "AB-ORGFO");

        // El riesgo empeora: se anota para quien ya la seguía; todos los seguidores reciben la foto nueva.
        assert_eq!(db.watch_apply("5569705329", &snap("bad", 0)), 2);
        let changed = &db.watch_list(ana)[0];
        assert_eq!((changed.change_kind.as_deref(), changed.change_from.as_deref(), changed.change_to.as_deref()), (Some("level"), Some("good"), Some("bad")));
        assert!(changed.changed_at.is_some());
        let late = &db.watch_list(luis)[0];
        assert_eq!((late.level.as_deref(), late.change_kind.clone()), (Some("bad"), None), "quien la sigue desde después no ve un cambio anterior a su primera revisión");

        // Cambio de estado del registro (p. ej. entra en concurso) con el mismo nivel de riesgo.
        db.watch_apply("5569705329", &snap("bad", 1));
        let status = &db.watch_list(ana)[0];
        assert_eq!((status.change_kind.as_deref(), status.change_from.as_deref(), status.change_to.as_deref()), (Some("status"), Some("0"), Some("1")));
        // "Sin valorar" es un nivel más: pasar de valorada a sin valorar también es un cambio.
        db.watch_apply("5569705329", &snap("", 1));
        assert_eq!(db.watch_list(ana)[0].change_to.as_deref(), Some(""));
    }

    #[test]
    fn deleting_a_user_keeps_the_table_consistent_and_history_lists_recent_things() {
        let db = Db::memory();
        let ana = db.create_user(&new_user("ana@x.co", Role::User)).unwrap();
        let log = |event: &str, detail: &str| {
            db.log(&NewActivity { user_id: Some(ana), user_label: "ana@x.co", role: Some(Role::User), event, method: "GET", path: "/", query: "", status: 200, ip: "", ua: "", detail });
        };
        log("company_view", "5569705329");
        log("search", "volvo");
        log("company_view", "5560125790");
        log("company_view", "5569705329"); // repetida: sale una vez, con la fecha más reciente
        log("search", "");
        let companies: Vec<String> = db.recent_companies(ana, 10).into_iter().map(|(o, _)| o).collect();
        assert_eq!(companies, ["5569705329", "5560125790"]);
        assert_eq!(db.recent_searches(ana, 10).len(), 1, "las búsquedas vacías no cuentan");
        assert_eq!(db.recent_companies(ana, 1).len(), 1);
        assert!(db.recent_companies(ana + 99, 10).is_empty());
    }

    #[test]
    fn initials() {
        let u = |n: &str| User { id: 1, email: None, username: Some("x".into()), name: n.into(), role: Role::User, active: true, lang: Lang::Es, must_change: false, created_at: String::new(), last_login: None, created_by: None };
        assert_eq!(u("Freddy Orozco").initials(), "FO");
        assert_eq!(u("ana").initials(), "A");
        assert_eq!(u("María del Carmen Ruiz").initials(), "MR");
    }
}
