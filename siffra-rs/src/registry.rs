//! Índice local del registro de empresas para buscar por nombre.
//!
//! La API gratuita de Bolagsverket solo consulta por número de organización. Bolagsverket publica además, cada
//! semana y sin coste, el archivo `bolagsverket_bulkfil.zip` con todas las organizaciones (valiosas datamängder).
//! Este módulo lo lee en streaming (≈1 GB descomprimido, ≈3 millones de filas), lo guarda en SQLite y crea un
//! índice de texto (FTS5) por nombre. Aquí NO se filtra ninguna fila: se carga todo lo que trae el archivo. Qué
//! se muestra (p. ej. excluir personas físicas) se decidirá en la consulta o en una fase posterior.
//!
//! Formato del archivo (UTF-8, una fila por línea, una por organización y número de protección de nombre):
//! `;` entre campos, cada campo entre comillas (`""` = comilla literal) y subcampos separados por `$`:
//! identidad `NÚMERO$TIPO`, nombres `NOMBRE$TIPO$FECHA` separados por `|`, forma jurídica, baja, etc.
//! No se guardan la descripción de la actividad ni la calle: la ficha en vivo (API) ya las trae.

use std::io::{BufRead, Read};
use std::path::Path;

use rusqlite::{params, Connection, OpenFlags};

/// Campos de una fila ya separados (sin las comillas).
pub type Fields = Vec<String>;

/// Lee las filas del archivo: una por línea, con campos entre comillas separados por `";"`. Llama a `f` por cada
/// fila (la cabecera se salta); si `f` devuelve `false` se detiene. Devuelve cuántas filas leyó.
///
/// El archivo real no lleva saltos de línea dentro de los campos, pero sí alguna comilla suelta en las
/// descripciones (p. ej. pulgadas), así que NO se interpreta el entrecomillado campo a campo: se separa por
/// la secuencia `";"`, que no aparece dentro del texto, y una comilla suelta no desordena la fila.
pub fn read_rows<R: BufRead>(mut reader: R, mut f: impl FnMut(Fields) -> bool) -> std::io::Result<u64> {
    let mut count = 0u64;
    let mut line = Vec::new();
    let mut first = true;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if first {
            first = false;
            continue; // cabecera
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\n', '\r']);
        if text.is_empty() {
            continue;
        }
        count += 1;
        if !f(split_fields(text)) {
            break;
        }
    }
    Ok(count)
}

fn split_fields(line: &str) -> Fields {
    let inner = line.strip_prefix('"').unwrap_or(line);
    let inner = inner.strip_suffix('"').unwrap_or(inner);
    inner.split("\";\"").map(|s| s.replace("\"\"", "\"")).collect()
}

/// Una fila del archivo, ya interpretada.
#[derive(Debug, PartialEq)]
pub struct Row {
    pub orgnr: String,
    /// `ORGNR-IDORG`, `PERSON-IDORG`, `DODSBO-IDORG`…
    pub id_type: String,
    pub seq: i64,
    /// Nombre principal (el primero de tipo `FORETAGSNAMN`, o el primero que haya).
    pub name: String,
    /// Todos los nombres (incluidos los especiales y de otros idiomas), separados por " | ".
    pub names: String,
    pub form: String,
    pub dereg: Option<String>,
    pub dereg_reason: Option<String>,
    pub registered: Option<String>,
    pub city: Option<String>,
    pub postcode: Option<String>,
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Interpreta los 11 campos. `None` si la fila no tiene la forma esperada.
pub fn parse_row(f: &[String]) -> Option<Row> {
    if f.len() < 11 {
        return None;
    }
    let (orgnr, id_type) = f[0].split_once('$')?;
    let mut name_list: Vec<(String, String)> = Vec::new();
    for part in f[3].split('|') {
        let mut it = part.split('$');
        let name = it.next().unwrap_or("").trim();
        let kind = it.next().unwrap_or("").trim();
        if !name.is_empty() {
            name_list.push((name.to_string(), kind.to_string()));
        }
    }
    let primary = name_list.iter().find(|(_, k)| k.starts_with("FORETAGSNAMN")).or_else(|| name_list.first())?;
    // postadress: gata $ c/o $ ORT $ postnummer $ land
    let addr: Vec<&str> = f[10].split('$').collect();
    Some(Row {
        orgnr: orgnr.trim().to_string(),
        id_type: id_type.trim().to_string(),
        seq: f[1].trim().parse().unwrap_or(1),
        name: primary.0.clone(),
        names: name_list.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(" | "),
        form: f[4].trim().to_string(),
        dereg: non_empty(&f[5]),
        dereg_reason: non_empty(f[6].split('$').next().unwrap_or("")),
        registered: non_empty(&f[8]),
        city: addr.get(2).and_then(|s| non_empty(s)),
        postcode: addr.get(3).and_then(|s| non_empty(s)),
    })
}

const SCHEMA: &str = "
CREATE TABLE company (
    id           INTEGER PRIMARY KEY,
    orgnr        TEXT NOT NULL,
    id_type      TEXT NOT NULL,
    seq          INTEGER NOT NULL,
    name         TEXT NOT NULL,
    names        TEXT NOT NULL,
    form         TEXT NOT NULL,
    dereg        TEXT,
    dereg_reason TEXT,
    registered   TEXT,
    city         TEXT,
    postcode     TEXT
);
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

const INDEXES: &str = "
CREATE INDEX company_orgnr ON company(orgnr);
CREATE VIRTUAL TABLE company_fts USING fts5(
    names, content='company', content_rowid='id',
    tokenize='unicode61 remove_diacritics 2', prefix='2 3', detail=column
);
INSERT INTO company_fts(company_fts) VALUES('rebuild');
";

#[derive(Debug, Default)]
pub struct ImportReport {
    pub read: u64,
    pub inserted: u64,
    pub skipped: u64,
}

/// Importa el archivo a una base NUEVA en `out` (se escribe en `out.part` y se renombra al terminar, así una
/// importación a medias nunca pisa el índice que está sirviendo). `limit` permite probar con pocas filas.
pub fn import<R: BufRead>(reader: R, out: &Path, limit: Option<u64>, source: &str, cache_kib: i64, mut progress: impl FnMut(u64)) -> Result<ImportReport, String> {
    let part = out.with_extension("db.part");
    for p in [&part, &part.with_extension("db.part-wal"), &part.with_extension("db.part-shm")] {
        let _ = std::fs::remove_file(p);
    }
    let mut conn = Connection::open(&part).map_err(|e| e.to_string())?;
    // Carga masiva: sin diario ni sincronización; si falla se descarta el archivo `.part` entero.
    conn.execute_batch(&format!("PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF; PRAGMA cache_size = -{cache_kib}; PRAGMA temp_store = FILE;"))
        .map_err(|e| e.to_string())?;
    conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;

    let mut report = ImportReport::default();
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare(
                "INSERT INTO company (orgnr, id_type, seq, name, names, form, dereg, dereg_reason, registered, city, postcode)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            )
            .map_err(|e| e.to_string())?;
        let mut err: Option<String> = None;
        read_rows(reader, |fields| {
            report.read += 1;
            match parse_row(&fields) {
                Some(r) => {
                    if let Err(e) = stmt.execute(params![r.orgnr, r.id_type, r.seq, r.name, r.names, r.form, r.dereg, r.dereg_reason, r.registered, r.city, r.postcode]) {
                        err = Some(e.to_string());
                        return false;
                    }
                    report.inserted += 1;
                }
                None => report.skipped += 1,
            }
            if report.read % 100_000 == 0 {
                progress(report.read);
            }
            limit.is_none_or(|l| report.read < l)
        })
        .map_err(|e| e.to_string())?;
        if let Some(e) = err {
            return Err(e);
        }
    }
    tx.commit().map_err(|e| e.to_string())?;

    progress(report.read);
    conn.execute_batch(INDEXES).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('source', ?1), ('imported_at', ?2), ('rows', ?3)",
        params![source, crate::util::now_iso(), report.inserted.to_string()],
    )
    .map_err(|e| e.to_string())?;
    conn.execute_batch("PRAGMA optimize;").map_err(|e| e.to_string())?;
    drop(conn);
    std::fs::rename(&part, out).map_err(|e| format!("no se pudo mover {} a {}: {e}", part.display(), out.display()))?;
    Ok(report)
}

/// Abre el archivo `.zip` del que sale `bolagsverket_bulkfil.txt` (o el `.txt` suelto) como lector en streaming.
pub fn open_source(path: &Path) -> Result<Box<dyn BufRead>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("no se pudo abrir {}: {e}", path.display()))?;
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
        let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
        // Se copia la entrada a un lector propio: `ZipFile` toma prestado el archivo, así que se carga por trozos.
        let name = (0..archive.len())
            .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
            .find(|n| n.to_lowercase().ends_with(".txt"))
            .ok_or("el zip no contiene ningún .txt")?;
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(8);
        std::thread::spawn(move || {
            let Ok(mut entry) = archive.by_name(&name) else { return };
            let mut buf = vec![0u8; 1 << 20];
            loop {
                match entry.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Ok(Box::new(std::io::BufReader::with_capacity(1 << 20, ChannelReader { rx, cur: Vec::new(), pos: 0 })))
    } else {
        Ok(Box::new(std::io::BufReader::with_capacity(1 << 20, file)))
    }
}

struct ChannelReader {
    rx: std::sync::mpsc::Receiver<Vec<u8>>,
    cur: Vec<u8>,
    pos: usize,
}

impl Read for ChannelReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.pos >= self.cur.len() {
            match self.rx.recv() {
                Ok(chunk) => {
                    self.cur = chunk;
                    self.pos = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.cur.len() - self.pos);
        out[..n].copy_from_slice(&self.cur[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

// ───────────────────────── Consulta ─────────────────────────

pub struct Registry {
    conn: std::sync::Mutex<Connection>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub orgnr: String,
    pub name: String,
    pub form: String,
    pub dereg: Option<String>,
    pub city: Option<String>,
    pub id_type: String,
}

#[derive(Debug, Default)]
pub struct Stats {
    pub rows: i64,
    pub distinct_orgnr: i64,
    pub active_rows: i64,
    pub by_id_type: Vec<(String, i64)>,
    pub by_form: Vec<(String, i64, i64)>, // forma, filas, activas
    pub imported_at: String,
    pub source: String,
}

/// Convierte lo que escribe la persona en una consulta FTS5: cada palabra es un prefijo y se exigen todas.
pub fn fts_query(input: &str) -> Option<String> {
    let tokens: Vec<String> = input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .take(8)
        .map(|t| format!("\"{}\"*", t.replace('"', "")))
        .collect();
    (!tokens.is_empty()).then(|| tokens.join(" "))
}

impl Registry {
    pub fn open(path: &Path) -> Result<Registry, String> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .map_err(|e| format!("no se pudo abrir el índice {}: {e}", path.display()))?;
        Ok(Registry { conn: std::sync::Mutex::new(conn) })
    }

    fn c(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Busca por nombre. Una empresa con varios nombres o varias filas sale una sola vez; las activas primero.
    pub fn search(&self, query: &str, limit: usize, include_deregistered: bool) -> Vec<Hit> {
        let Some(q) = fts_query(query) else { return vec![] };
        let conn = self.c();
        let sql = format!(
            "SELECT c.orgnr, c.name, c.form, c.dereg, c.city, c.id_type
             FROM company_fts f JOIN company c ON c.id = f.rowid
             WHERE company_fts MATCH ?1 {}
             ORDER BY (c.dereg IS NULL) DESC, rank
             LIMIT ?2",
            if include_deregistered { "" } else { "AND c.dereg IS NULL" }
        );
        let Ok(mut stmt) = conn.prepare(&sql) else { return vec![] };
        let rows = stmt
            .query_map(params![q, (limit * 4) as i64], |r| {
                Ok(Hit { orgnr: r.get(0)?, name: r.get(1)?, form: r.get(2)?, dereg: r.get(3)?, city: r.get(4)?, id_type: r.get(5)? })
            })
            .map(|it| it.flatten().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        rows.into_iter().filter(|h| seen.insert(h.orgnr.clone())).take(limit).collect()
    }

    /// Número de filas del índice, leído de los metadatos (instantáneo; `stats()` recorre las tres millones de filas).
    pub fn row_count(&self) -> i64 {
        self.c().query_row("SELECT value FROM meta WHERE key='rows'", [], |r| r.get::<_, String>(0)).ok().and_then(|v| v.parse().ok()).unwrap_or(0)
    }

    pub fn stats(&self) -> Stats {
        let conn = self.c();
        let one = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };
        let meta = |k: &str| -> String { conn.query_row("SELECT value FROM meta WHERE key=?1", [k], |r| r.get(0)).unwrap_or_default() };
        let by_id_type = conn
            .prepare("SELECT id_type, COUNT(*) FROM company GROUP BY id_type ORDER BY 2 DESC")
            .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map(|it| it.flatten().collect()))
            .unwrap_or_default();
        let by_form = conn
            .prepare("SELECT form, COUNT(*), SUM(dereg IS NULL) FROM company GROUP BY form ORDER BY 2 DESC")
            .and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map(|it| it.flatten().collect()))
            .unwrap_or_default();
        Stats {
            rows: one("SELECT COUNT(*) FROM company"),
            distinct_orgnr: one("SELECT COUNT(DISTINCT orgnr) FROM company"),
            active_rows: one("SELECT COUNT(*) FROM company WHERE dereg IS NULL"),
            by_id_type,
            by_form,
            imported_at: meta("imported_at"),
            source: meta("source"),
        }
    }
}

// ───────────────────────── Índice vivo y actualización semanal ─────────────────────────

/// URL oficial del archivo de Bolagsverket (se actualiza cada semana).
pub const BULK_URL: &str = "https://vardefulla-datamangder.bolagsverket.se/bolagsverket/bolagsverket_bulkfil.zip";
/// Pasada esta edad, el servidor vuelve a descargar el archivo.
pub const MAX_AGE: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 3600);

/// El índice que está sirviendo. Se puede cambiar por uno nuevo sin parar el servidor: las búsquedas en curso
/// terminan con el anterior y las siguientes usan el nuevo.
pub struct RegistryHandle {
    path: std::path::PathBuf,
    current: std::sync::RwLock<Option<std::sync::Arc<Registry>>>,
    refreshing: std::sync::atomic::AtomicBool,
}

impl RegistryHandle {
    /// Abre el índice de `path` si existe; si no, queda vacío (la búsqueda por nombre no está disponible).
    pub fn new(path: std::path::PathBuf) -> std::sync::Arc<RegistryHandle> {
        let h = std::sync::Arc::new(RegistryHandle { path, current: std::sync::RwLock::new(None), refreshing: std::sync::atomic::AtomicBool::new(false) });
        h.reload();
        h
    }

    /// Sin índice (pruebas, o servidor sin archivo cargado).
    pub fn none() -> std::sync::Arc<RegistryHandle> {
        RegistryHandle::new(std::path::PathBuf::new())
    }

    pub fn reload(&self) -> bool {
        match Registry::open(&self.path) {
            Ok(r) => {
                *self.current.write().unwrap_or_else(|e| e.into_inner()) = Some(std::sync::Arc::new(r));
                true
            }
            Err(_) => false,
        }
    }

    pub fn get(&self) -> Option<std::sync::Arc<Registry>> {
        self.current.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Antigüedad del archivo del índice, o `None` si no existe.
    pub fn age(&self) -> Option<std::time::Duration> {
        std::fs::metadata(&self.path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok())
    }
}

/// Descarga el archivo de `url`, construye un índice nuevo y lo pone en servicio. Mientras tanto el índice
/// anterior sigue respondiendo; si algo falla, nada cambia. `min_bytes` descarta descargas truncadas.
pub async fn refresh(handle: &std::sync::Arc<RegistryHandle>, url: &str, min_bytes: u64) -> Result<ImportReport, String> {
    use std::sync::atomic::Ordering;
    if handle.refreshing.swap(true, Ordering::SeqCst) {
        return Err("ya hay una actualización en curso".into());
    }
    let result = refresh_inner(handle, url, min_bytes).await;
    handle.refreshing.store(false, Ordering::SeqCst);
    result
}

async fn refresh_inner(handle: &std::sync::Arc<RegistryHandle>, url: &str, min_bytes: u64) -> Result<ImportReport, String> {
    use std::io::Write;
    let dir = handle.path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
    let bulk = dir.join("bulk");
    std::fs::create_dir_all(&bulk).map_err(|e| format!("no se pudo crear {}: {e}", bulk.display()))?;
    let (part, zip_path) = (bulk.join("bolagsverket_bulkfil.zip.part"), bulk.join("bolagsverket_bulkfil.zip"));

    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(1800)).build().map_err(|e| e.to_string())?;
    let mut resp = client.get(url).send().await.map_err(|e| format!("descarga: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("descarga: HTTP {}", resp.status()));
    }
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut total = 0u64;
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("descarga: {e}"))? {
        total += chunk.len() as u64;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
    }
    drop(file);
    if total < min_bytes {
        let _ = std::fs::remove_file(&part);
        return Err(format!("la descarga ({total} bytes) es más pequeña de lo esperado ({min_bytes})"));
    }
    std::fs::rename(&part, &zip_path).map_err(|e| e.to_string())?;

    let (out, zip2) = (handle.path.clone(), zip_path.clone());
    // Caché pequeña (32 MiB): el servidor no debe crecer en memoria mientras importa.
    let report = tokio::task::spawn_blocking(move || {
        let reader = open_source(&zip2)?;
        import(reader, &out, None, "bolagsverket_bulkfil.zip", 32_000, |_| {})
    })
    .await
    .map_err(|e| e.to_string())??;
    let _ = std::fs::remove_file(&zip_path); // el zip ya no hace falta: 250 MB menos en disco
    handle.reload();
    Ok(report)
}

/// Tarea de fondo: cada 6 horas comprueba la edad del índice y, si falta o tiene más de una semana, lo renueva.
pub fn spawn_auto_refresh(handle: std::sync::Arc<RegistryHandle>) {
    tokio::spawn(async move {
        loop {
            if handle.age().is_none_or(|a| a > MAX_AGE) {
                println!("Índice de empresas: actualizando desde Bolagsverket…");
                match refresh(&handle, BULK_URL, 50_000_000).await {
                    Ok(r) => println!("Índice de empresas actualizado: {} filas", r.inserted),
                    Err(e) => eprintln!("Índice de empresas: la actualización falló ({e}); se conserva el anterior"),
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
        }
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Líneas con la forma real del archivo: comillas dobladas, varios nombres, persona física y una comilla suelta
    /// en la descripción (en el archivo real hay decenas de miles de líneas así de raras).
    pub(crate) const SAMPLE: &str = "organisationsidentitet;namnskyddslopnummer;registreringsland;organisationsnamn;organisationsform;avregistreringsdatum;avregistreringsorsak;pagandeAvvecklingsEllerOmstruktureringsforfarande;registreringsdatum;verksamhetsbeskrivning;postadress
\"5560000019$ORGNR-IDORG\";\"1\";\"SE-LAND\";\"Nordlys Logistik AB$FORETAGSNAMN-ORGNAM$2001-02-03|Nordlys Logistics$ANNATSPRAK-ORGNAM$2005-01-01\";\"AB-ORGFO\";\"\";\"\";\"\";\"2001-02-03\";\"Transport och \"\"logistik\"\".\";\"Storgatan 1$$GÖTEBORG$41101$SE-LAND\"
\"199001019999$PERSON-IDORG\";\"1\";\"SE-LAND\";\"Åsa Östlund$FORETAGSNAMN-ORGNAM$1999-01-01\";\"E-ORGFO\";\"2010-10-14\";\"VERKUPP-AVORG\";\"\";\"1999-01-01\";\"Café med skärmar på 24 tum (24\" ) och mer\";\"Box 4$c/o Någon$ÖSTERSUND$83100$SE-LAND\"
\"5560000027$ORGNR-IDORG\";\"2\";\"SE-LAND\";\"Fjällbruk Bygg & Design AB$FORETAGSNAMN-ORGNAM$1990-05-05\";\"AB-ORGFO\";\"\";\"\";\"\";\"1990-05-05\";\"\";\"Väg 2$$ÖSTERSUND$83101$SE-LAND\"
";

    #[test]
    fn reads_one_row_per_line_even_with_a_stray_quote() {
        let mut rows = Vec::new();
        let n = read_rows(SAMPLE.as_bytes(), |f| {
            rows.push(f);
            true
        })
        .unwrap();
        assert_eq!(n, 3, "la cabecera no cuenta");
        assert!(rows.iter().all(|f| f.len() == 11), "una comilla suelta no desordena los campos");
        assert_eq!(rows[0][9], "Transport och \"logistik\".");
        assert_eq!(rows[1][9], "Café med skärmar på 24 tum (24\" ) och mer");
        assert_eq!(rows[1][10], "Box 4$c/o Någon$ÖSTERSUND$83100$SE-LAND");
    }

    #[test]
    fn parses_names_dates_and_address() {
        let mut rows = Vec::new();
        read_rows(SAMPLE.as_bytes(), |f| {
            rows.push(parse_row(&f).unwrap());
            true
        })
        .unwrap();
        let ab = &rows[0];
        assert_eq!((ab.orgnr.as_str(), ab.id_type.as_str(), ab.seq, ab.form.as_str()), ("5560000019", "ORGNR-IDORG", 1, "AB-ORGFO"));
        assert_eq!(ab.name, "Nordlys Logistik AB");
        assert_eq!(ab.names, "Nordlys Logistik AB | Nordlys Logistics");
        assert_eq!((ab.dereg.as_deref(), ab.city.as_deref(), ab.postcode.as_deref()), (None, Some("GÖTEBORG"), Some("41101")));
        let person = &rows[1];
        assert_eq!(person.id_type, "PERSON-IDORG");
        assert_eq!((person.dereg.as_deref(), person.dereg_reason.as_deref()), (Some("2010-10-14"), Some("VERKUPP-AVORG")));
        assert_eq!(rows[2].seq, 2);
    }

    #[test]
    fn import_then_search_by_name_prefix_and_without_diacritics() {
        let dir = std::env::temp_dir().join(format!("siffra-registry-{}", crate::util::random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("registry.db");
        let report = import(SAMPLE.as_bytes(), &out, None, "muestra", 8_000, |_| {}).unwrap();
        assert_eq!((report.read, report.inserted, report.skipped), (3, 3, 0));
        assert!(out.exists() && !out.with_extension("db.part").exists(), "se renombra al terminar");

        let reg = Registry::open(&out).unwrap();
        let hit = |q: &str| reg.search(q, 10, true).into_iter().map(|h| h.orgnr).collect::<Vec<_>>();
        assert_eq!(hit("nordlys"), ["5560000019"]);
        assert_eq!(hit("Nord log"), ["5560000019"], "cada palabra es un prefijo y se exigen todas");
        assert_eq!(hit("logistics"), ["5560000019"], "también por el nombre en otro idioma");
        assert_eq!(hit("fjallbruk"), ["5560000027"], "sin diacríticos encuentra Fjällbruk");
        assert_eq!(hit("asa ostlund"), ["199001019999"]);
        assert!(hit("zzz").is_empty());
        // Las dadas de baja solo salen si se piden.
        assert!(reg.search("ostlund", 10, false).is_empty());
        assert_eq!(reg.search("ostlund", 10, true).len(), 1);

        let s = reg.stats();
        assert_eq!((s.rows, s.distinct_orgnr, s.active_rows), (3, 3, 2));
        assert_eq!(s.by_id_type[0], ("ORGNR-IDORG".to_string(), 2));
        assert_eq!(s.source, "muestra");
        drop(reg);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_respects_limit_and_never_leaves_a_half_written_index() {
        let dir = std::env::temp_dir().join(format!("siffra-registry-{}", crate::util::random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("r.db");
        let report = import(SAMPLE.as_bytes(), &out, Some(2), "m", 8_000, |_| {}).unwrap();
        assert_eq!(report.inserted, 2);
        assert_eq!(Registry::open(&out).unwrap().stats().rows, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fts_queries_are_sanitised() {
        assert_eq!(fts_query("Volvo AB").as_deref(), Some("\"Volvo\"* \"AB\"*"));
        assert_eq!(fts_query("  \" OR 1=1 -- ").as_deref(), Some("\"OR\"* \"1\"* \"1\"*"));
        assert_eq!(fts_query("***"), None);
    }
}
