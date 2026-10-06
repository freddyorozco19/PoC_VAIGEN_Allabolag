//! Segunda fuente de cuentas anuales, solo como complemento: los informes ESEF de las empresas cotizadas.
//!
//! La API gratuita de Bolagsverket solo entrega los informes presentados en formato digital (iXBRL de pymes).
//! Las cotizadas (Ericsson, Volvo, H&M…) presentan en ESEF a Finansinspektionen y quedan indexadas, con sus cifras
//! en xBRL-JSON y la taxonomía IFRS, en el repositorio público de XBRL International (filings.xbrl.org).
//! Ese repositorio identifica a cada empresa por su LEI; el registro público GLEIF lo relaciona con el número de
//! organización.
//!
//! Solo se consulta cuando Bolagsverket NO tiene informes de la empresa: nunca sustituye ni modifica lo que ya
//! se lee de Bolagsverket. Las cifras son las del GRUPO (consolidadas) y solo se aceptan informes en coronas.
//! Se guardan con el mismo almacén (`report` + `fact`), con identificadores `esef-<LEI>-<fin del ejercicio>`.
//!
//! Variables de entorno (opcionales): `SIFFRA_ESEF=0` apaga esta fuente; `SIFFRA_GLEIF_URL` y
//! `SIFFRA_XBRL_URL` cambian las direcciones (en las pruebas apuntan a servidores simulados; sin ellas, las
//! pruebas no hacen ninguna petición de red).

use std::sync::LazyLock;
use std::time::Duration;

use serde_json::Value;

use crate::annual_report::{pick_reports, RawFact, Store};
use crate::bolagsverket::DocRef;
use crate::util;

const DEFAULT_GLEIF: &str = "https://api.gleif.org/api/v1";
const DEFAULT_FILINGS: &str = "https://filings.xbrl.org";
/// Los grupos grandes pesan bastante (Volvo ≈ 17 MB en JSON).
const MAX_JSON_BYTES: usize = 40 * 1024 * 1024;
/// Autoridad de registro de Bolagsverket en la lista de GLEIF.
const SWEDEN_RA: &str = "RA000544";
/// Cada cuántos días se vuelve a mirar si hay un informe nuevo (los ya guardados no se descargan otra vez).
const REFRESH_DAYS: i64 = 7;
const MAX_FACT_TEXT: usize = 20_000;

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder().timeout(Duration::from_secs(90)).user_agent("siffra-rs/0.1 (PoC)").build().expect("cliente HTTP")
});

/// Dirección base de un servicio, o `None` si la fuente está apagada (o es una prueba sin servidor simulado).
fn endpoint(var: &str, default: &str) -> Option<String> {
    if std::env::var("SIFFRA_ESEF").map(|v| v.trim() == "0").unwrap_or(false) {
        return None;
    }
    match std::env::var(var) {
        Ok(v) if !v.trim().is_empty() => Some(v.trim().trim_end_matches('/').to_string()),
        _ => (!cfg!(test)).then(|| default.to_string()),
    }
}

/// Prefijo de los identificadores de documento de esta fuente.
pub const DOC_PREFIX: &str = "esef-";

/// ¿El informe guardado con este identificador viene de ESEF?
pub fn is_esef_doc(doc_id: &str) -> bool {
    doc_id.starts_with(DOC_PREFIX)
}

// ───────────── xBRL-JSON → hechos ─────────────

/// Quita las etiquetas HTML de un texto del informe y junta los espacios.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len().min(MAX_FACT_TEXT * 2));
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
        if out.len() > MAX_FACT_TEXT * 2 {
            break;
        }
    }
    let text = out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">");
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_FACT_TEXT).collect()
}

/// Fecha de un periodo xBRL-JSON. Las fechas finales e instantes vienen como inicio del día siguiente
/// (`2025-01-01T00:00:00` = 31 de diciembre): con `end` se devuelve el último día del periodo.
fn date_of(s: &str, end: bool) -> Option<String> {
    let date = s.get(..10)?;
    let time = s.get(11..).unwrap_or("");
    if end && (time.is_empty() || time.starts_with("00:00:00")) && s.contains('T') {
        let t = util::unix_from_iso(date)?;
        return Some(util::iso_from_unix(t - 86_400).get(..10)?.to_string());
    }
    util::unix_from_iso(date).map(|_| date.to_string())
}

/// Algunos informes (p. ej. Handelsbanken) fechan el saldo de apertura del patrimonio el 2 de enero a las 00:00,
/// que tras restar un día es el 1 de enero: es el cierre del ejercicio anterior (31 de diciembre).
fn opening_balance_to_year_end(date: String) -> String {
    if date.ends_with("-01-01") {
        if let Some(t) = util::unix_from_iso(&date) {
            return util::iso_from_unix(t - 86_400).get(..10).unwrap_or(&date).to_string();
        }
    }
    date
}

fn unit_label(unit: &str) -> String {
    unit.split('/').map(|u| u.rsplit(':').next().unwrap_or(u)).collect::<Vec<_>>().join("/")
}

/// Convierte un informe xBRL-JSON en los mismos `RawFact` que produce el lector de iXBRL de Bolagsverket.
/// Falla si el informe no está en coronas (las cifras se mostrarían como SEK siendo de otra moneda).
pub fn parse_xbrl_json(bytes: &[u8]) -> Result<Vec<RawFact>, String> {
    let root: Value = serde_json::from_slice(bytes).map_err(|e| format!("xBRL-JSON no válido: {e}"))?;
    let facts = root.get("facts").and_then(Value::as_object).ok_or("xBRL-JSON sin hechos")?;

    let mut out = Vec::with_capacity(facts.len());
    let (mut sek, mut other_currency) = (0usize, 0usize);
    for (id, fact) in facts {
        let Some(dims) = fact.get("dimensions").and_then(Value::as_object) else { continue };
        let Some(concept) = dims.get("concept").and_then(Value::as_str) else { continue };
        let unit = dims.get("unit").and_then(Value::as_str).map(unit_label);
        let (mut instant, mut start, mut end) = (None, None, None);
        if let Some(period) = dims.get("period").and_then(Value::as_str) {
            match period.split_once('/') {
                Some((a, b)) => {
                    start = date_of(a, false);
                    end = date_of(b, true);
                }
                None => instant = date_of(period, true).map(opening_balance_to_year_end),
            }
        }
        let mut axes: Vec<String> = dims
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "concept" | "entity" | "period" | "unit" | "language"))
            .map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or("")))
            .collect();
        axes.sort();

        let raw = fact.get("value");
        let (value, text) = if unit.is_some() {
            let v = match raw {
                Some(Value::Number(n)) => n.as_f64(),
                Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
                _ => None,
            };
            if let Some(u) = &unit {
                if u.starts_with("SEK") && !u.contains('/') {
                    sek += 1;
                } else if v.is_some() && !u.contains('/') && u.len() == 3 && u.chars().all(|c| c.is_ascii_uppercase()) {
                    other_currency += 1;
                }
            }
            (v, raw.and_then(Value::as_str).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
        } else {
            (None, raw.and_then(Value::as_str).map(strip_tags).filter(|s| !s.is_empty()))
        };
        out.push(RawFact { concept: concept.to_string(), ctx: id.clone(), value, text, unit, scale: 0, instant, start, end, dims: axes.join(";") });
    }
    if out.is_empty() {
        return Err("el informe no trae hechos".to_string());
    }
    if other_currency > sek {
        return Err("el informe no está en coronas suecas".to_string());
    }
    Ok(out)
}

// ───────────── GLEIF y filings.xbrl.org ─────────────

fn valid_lei(lei: &str) -> bool {
    lei.len() == 20 && lei.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// LEI de una organización sueca (10 dígitos) a partir de su número de organización, o `None` si no tiene.
async fn lei_for(gleif: &str, orgnr: &str) -> Result<Option<String>, String> {
    if orgnr.len() != 10 || !orgnr.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    // GLEIF guarda el número tal como lo escribió quien pidió el LEI: unos con guion (556016-0680), otros sin él
    // (5560427220, p. ej. H&M o Atlas Copco). Se prueban las dos formas.
    let forms = [format!("{}-{}", &orgnr[..6], &orgnr[6..]), orgnr.to_string()];
    let mut best: Option<(bool, String)> = None;
    for form in &forms {
        let res = CLIENT
            .get(format!("{gleif}/lei-records"))
            .query(&[("filter[entity.registeredAs]", form.as_str()), ("page[size]", "10")])
            .header("Accept", "application/vnd.api+json")
            .send()
            .await
            .map_err(|e| format!("GLEIF: {e}"))?;
        if !res.status().is_success() {
            return Err(format!("GLEIF: error HTTP {}", res.status().as_u16()));
        }
        let body: Value = res.json().await.map_err(|e| format!("GLEIF: respuesta no válida: {e}"))?;
        for rec in body.get("data").and_then(Value::as_array).into_iter().flatten() {
            let (Some(lei), Some(entity)) = (rec.get("id").and_then(Value::as_str), rec.pointer("/attributes/entity")) else { continue };
            let registered_in_sweden = entity.pointer("/registeredAt/id").and_then(Value::as_str) == Some(SWEDEN_RA);
            let registered_as = entity.get("registeredAs").and_then(Value::as_str).unwrap_or("").replace('-', "");
            if !valid_lei(lei) || !registered_in_sweden || registered_as != orgnr {
                continue;
            }
            let issued = rec.pointer("/attributes/registration/status").and_then(Value::as_str) == Some("ISSUED");
            if best.as_ref().map(|(i, _)| !*i && issued).unwrap_or(true) {
                best = Some((issued, lei.to_string()));
            }
        }
        if best.is_some() {
            break;
        }
    }
    Ok(best.map(|(_, lei)| lei))
}

struct Filing {
    period_end: String,
    registered: String,
    json_url: String,
}

/// Informes ESEF suecos de una empresa que tienen versión xBRL-JSON.
async fn list_filings(base: &str, lei: &str) -> Result<Vec<Filing>, String> {
    if !valid_lei(lei) {
        return Ok(vec![]);
    }
    let res = CLIENT.get(format!("{base}/api/entities/{lei}/filings")).send().await.map_err(|e| format!("filings.xbrl.org: {e}"))?;
    if res.status().as_u16() == 404 {
        return Ok(vec![]);
    }
    if !res.status().is_success() {
        return Err(format!("filings.xbrl.org: error HTTP {}", res.status().as_u16()));
    }
    let body: Value = res.json().await.map_err(|e| format!("filings.xbrl.org: respuesta no válida: {e}"))?;
    let mut out = Vec::new();
    for f in body.get("data").and_then(Value::as_array).into_iter().flatten() {
        let a = f.get("attributes");
        let get = |k: &str| a.and_then(|a| a.get(k)).and_then(Value::as_str);
        let (Some(period_end), Some(json_url)) = (get("period_end"), get("json_url")) else { continue };
        // Solo rutas relativas del propio repositorio.
        if get("country") != Some("SE") || !json_url.starts_with('/') || json_url.contains("..") || json_url.contains("//") {
            continue;
        }
        out.push(Filing { period_end: period_end.get(..10).unwrap_or(period_end).to_string(), registered: get("date_added").and_then(|d| d.get(..10)).unwrap_or("").to_string(), json_url: json_url.to_string() });
    }
    Ok(out)
}

async fn download_json(base: &str, path: &str) -> Result<Vec<u8>, String> {
    let res = CLIENT.get(format!("{base}{path}")).send().await.map_err(|e| format!("filings.xbrl.org: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("filings.xbrl.org: error HTTP {} al bajar el informe", res.status().as_u16()));
    }
    if res.content_length().map(|n| n as usize > MAX_JSON_BYTES).unwrap_or(false) {
        return Err("el informe ESEF es demasiado grande".to_string());
    }
    let bytes = res.bytes().await.map_err(|e| format!("filings.xbrl.org: {e}"))?;
    if bytes.len() > MAX_JSON_BYTES {
        return Err("el informe ESEF es demasiado grande".to_string());
    }
    Ok(bytes.to_vec())
}

// ───────────── Carga ─────────────

/// Informes ESEF de una organización, del más reciente al más antiguo (hasta 3 que cubren 5 ejercicios).
/// `Ok(vec![])` si no hay (o la fuente está apagada); `Err` solo ante un fallo de red/servicio: quien llama
/// no debe recordar ese resultado como "no tiene informes".
pub async fn load_reports(store: Option<&Store>, orgnr: &str) -> Result<Vec<Vec<RawFact>>, String> {
    let (Some(gleif), Some(filings_base)) = (endpoint("SIFFRA_GLEIF_URL", DEFAULT_GLEIF), endpoint("SIFFRA_XBRL_URL", DEFAULT_FILINGS)) else {
        return Ok(vec![]);
    };

    let stored: Vec<DocRef> = store
        .map(|s| s.db.reports_of(orgnr).into_iter().filter(|r| is_esef_doc(&r.doc_id)).map(|r| DocRef { id: r.doc_id, period_end: r.period_end, registered: r.registered }).collect())
        .unwrap_or_default();
    // De lo guardado se sirve TODO (más reciente primero): incluye los informes extra que se bajaron para tapar huecos.
    let from_store = |mut docs: Vec<DocRef>| -> Vec<Vec<RawFact>> {
        docs.sort_by(|a, b| b.period_end.cmp(&a.period_end).then(b.registered.cmp(&a.registered)));
        docs.dedup_by(|b, a| a.period_end == b.period_end);
        docs.iter().take(MAX_STORED_REPORTS).filter_map(|d| store.and_then(|s| s.db.report_facts(&d.id))).collect()
    };
    let newest_fetch = store.and_then(|s| s.db.reports_of(orgnr).into_iter().filter(|r| is_esef_doc(&r.doc_id)).map(|r| r.fetched_at).max());
    if let Some(at) = &newest_fetch {
        if util::days_since(at) < REFRESH_DAYS {
            return Ok(from_store(stored));
        }
    }

    match fetch(store, &gleif, &filings_base, orgnr).await {
        Ok(reports) => Ok(reports),
        // Sin red pero con informes ya guardados: se sirven esos.
        Err(e) if !stored.is_empty() => {
            eprintln!("ESEF {orgnr}: {e} (se usan los informes guardados)");
            Ok(from_store(stored))
        }
        Err(e) => Err(e),
    }
}

/// Máximo de informes ESEF que se combinan (5 ejercicios).
const MAX_STORED_REPORTS: usize = 5;

/// Lee un informe (de lo guardado o bajándolo) y lo guarda. `Ok(None)` si no se puede usar (ilegible, otra moneda…).
async fn load_one(store: Option<&Store>, base: &str, orgnr: &str, filings: &[Filing], doc: &DocRef, essential: bool) -> Result<Option<Vec<RawFact>>, String> {
    if let Some(facts) = store.and_then(|s| s.db.report_facts(&doc.id)) {
        return Ok(Some(facts));
    }
    // Entre varias versiones del mismo ejercicio (sueco/inglés, reenvíos) gana la registrada más tarde.
    let Some(filing) = filings.iter().filter(|f| f.period_end == doc.period_end).max_by(|a, b| a.registered.cmp(&b.registered)) else { return Ok(None) };
    let bytes = match download_json(base, &filing.json_url).await {
        Ok(b) => b,
        Err(e) if essential => return Err(e),
        Err(e) => {
            eprintln!("ESEF {orgnr} {}: {e}", doc.period_end);
            return Ok(None);
        }
    };
    match parse_xbrl_json(&bytes) {
        Ok(facts) => {
            if let Some(s) = store {
                s.save_esef(orgnr, doc, &bytes, &facts);
            }
            Ok(Some(facts))
        }
        // Un informe ilegible o en otra moneda no se muestra, pero tampoco hace fallar la ficha.
        Err(e) => {
            eprintln!("ESEF {orgnr} {}: {e}", doc.period_end);
            Ok(None)
        }
    }
}

/// Ejercicios entre los cubiertos que quedaron sin ninguna cifra clave (p. ej. el informe más reciente trae el año
/// anterior con errores de transformación XBRL): hay que leer el informe de ese propio ejercicio.
fn empty_years(reports: &[(String, Vec<RawFact>)]) -> Vec<String> {
    let mut sorted: Vec<&(String, Vec<RawFact>)> = reports.iter().collect();
    sorted.sort_by(|a, b| b.0.cmp(&a.0));
    let facts: Vec<Vec<RawFact>> = sorted.iter().map(|(_, f)| f.clone()).collect();
    crate::annual_report::merge_raw(&facts)
        .years
        .iter()
        .filter(|y| y.revenue.is_none() && y.result.is_none() && y.assets.is_none() && y.equity.is_none())
        .map(|y| y.period_end.clone())
        .collect()
}

async fn fetch(store: Option<&Store>, gleif: &str, filings_base: &str, orgnr: &str) -> Result<Vec<Vec<RawFact>>, String> {
    let Some(lei) = lei_for(gleif, orgnr).await? else { return Ok(vec![]) };
    let filings = list_filings(filings_base, &lei).await?;
    let docs: Vec<DocRef> = filings.iter().map(|f| DocRef { id: format!("{DOC_PREFIX}{lei}-{}", f.period_end), period_end: f.period_end.clone(), registered: f.registered.clone() }).collect();
    let picked = pick_reports(docs.clone());

    let mut loaded: Vec<(String, Vec<RawFact>)> = Vec::new();
    for (i, doc) in picked.iter().enumerate() {
        if let Some(facts) = load_one(store, filings_base, orgnr, &filings, doc, i == 0).await? {
            loaded.push((doc.period_end.clone(), facts));
        }
    }
    // Huecos: un ejercicio sin cifras que tiene su propio informe en el índice se lee también.
    for year in empty_years(&loaded) {
        if loaded.len() >= MAX_STORED_REPORTS || loaded.iter().any(|(p, _)| *p == year) {
            continue;
        }
        let Some(doc) = pick_one(&docs, &year) else { continue };
        if let Some(facts) = load_one(store, filings_base, orgnr, &filings, doc, false).await? {
            loaded.push((year, facts));
        }
    }
    loaded.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(loaded.into_iter().map(|(_, f)| f).collect())
}

/// El documento de un ejercicio concreto (el registrado más tarde).
fn pick_one<'a>(docs: &'a [DocRef], period_end: &str) -> Option<&'a DocRef> {
    docs.iter().filter(|d| d.period_end == period_end).max_by(|a, b| a.registered.cmp(&b.registered))
}


// ───────────── Grupo al que pertenece una sociedad ─────────────

/// Sociedad matriz última de una organización (según GLEIF), para enlazar a las cifras consolidadas del grupo.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub name: String,
    /// Código de país ISO de la sede legal ("SE", "LU"…).
    pub country: String,
    /// Número de organización (10 dígitos) si la matriz está registrada en Bolagsverket.
    pub orgnr: Option<String>,
    /// La matriz tiene informes ESEF en el índice: su ficha trae cifras consolidadas.
    pub has_report: bool,
}

const GROUP_TTL: Duration = Duration::from_secs(6 * 60 * 60);
type GroupCache = std::collections::HashMap<String, (std::time::Instant, Option<Group>)>;
static GROUPS: LazyLock<std::sync::Mutex<GroupCache>> = LazyLock::new(|| std::sync::Mutex::new(GroupCache::new()));

/// Lo que ya se sabe del grupo de una organización, sin consultar la red (la ficha lo pinta si está).
pub fn cached_group(orgnr: &str) -> Option<Group> {
    let guard = GROUPS.lock().unwrap_or_else(|e| e.into_inner());
    guard.get(orgnr).filter(|(at, _)| at.elapsed() < GROUP_TTL).and_then(|(_, g)| g.clone())
}

/// Busca la matriz última de una organización en GLEIF y la recuerda 6 horas. `Ok(None)`: no pertenece a un grupo
/// que declare su matriz (o la fuente está apagada). Un fallo de red no se recuerda.
pub async fn group_of(orgnr: &str) -> Result<Option<Group>, String> {
    if let Some((at, g)) = GROUPS.lock().unwrap_or_else(|e| e.into_inner()).get(orgnr) {
        if at.elapsed() < GROUP_TTL {
            return Ok(g.clone());
        }
    }
    let (Some(gleif), Some(filings_base)) = (endpoint("SIFFRA_GLEIF_URL", DEFAULT_GLEIF), endpoint("SIFFRA_XBRL_URL", DEFAULT_FILINGS)) else {
        return Ok(None);
    };
    let group = find_group(&gleif, &filings_base, orgnr).await?;
    GROUPS.lock().unwrap_or_else(|e| e.into_inner()).insert(orgnr.to_string(), (std::time::Instant::now(), group.clone()));
    Ok(group)
}

async fn find_group(gleif: &str, filings_base: &str, orgnr: &str) -> Result<Option<Group>, String> {
    let Some(lei) = lei_for(gleif, orgnr).await? else { return Ok(None) };
    let res = CLIENT.get(format!("{gleif}/lei-records/{lei}/ultimate-parent")).header("Accept", "application/vnd.api+json").send().await.map_err(|e| format!("GLEIF: {e}"))?;
    // 404: sin relación de matriz declarada.
    if res.status().as_u16() == 404 {
        return Ok(None);
    }
    if !res.status().is_success() {
        return Err(format!("GLEIF: error HTTP {}", res.status().as_u16()));
    }
    let body: Value = res.json().await.map_err(|e| format!("GLEIF: respuesta no válida: {e}"))?;
    let Some(parent) = body.get("data").filter(|d| d.is_object()) else { return Ok(None) };
    let parent_lei = parent.get("id").and_then(Value::as_str).unwrap_or("");
    let entity = parent.pointer("/attributes/entity");
    let Some(name) = entity.and_then(|e| e.pointer("/legalName/name")).and_then(Value::as_str) else { return Ok(None) };
    if parent_lei == lei || !valid_lei(parent_lei) {
        return Ok(None);
    }
    let country = entity.and_then(|e| e.pointer("/legalAddress/country")).and_then(Value::as_str).unwrap_or("").to_string();
    let swedish = entity.and_then(|e| e.pointer("/registeredAt/id")).and_then(Value::as_str) == Some(SWEDEN_RA);
    let parent_orgnr = entity
        .and_then(|e| e.get("registeredAs"))
        .and_then(Value::as_str)
        .map(|r| r.replace('-', ""))
        .filter(|r| swedish && r.len() == 10 && r.bytes().all(|b| b.is_ascii_digit()) && r != orgnr);
    let has_report = match &parent_orgnr {
        Some(_) => !list_filings(filings_base, parent_lei).await.unwrap_or_default().is_empty(),
        None => false,
    };
    Ok(Some(Group { name: name.to_string(), country, orgnr: parent_orgnr, has_report }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_ends_move_back_one_day_and_starts_do_not() {
        assert_eq!(date_of("2025-01-01T00:00:00", true).as_deref(), Some("2024-12-31"));
        assert_eq!(date_of("2024-01-01T00:00:00", false).as_deref(), Some("2024-01-01"));
        assert_eq!(date_of("2024-03-01T00:00:00", true).as_deref(), Some("2024-02-29"), "año bisiesto");
        assert_eq!(date_of("2024-12-31", true).as_deref(), Some("2024-12-31"), "sin hora no se mueve");
        assert_eq!(date_of("nonsense", true), None);
    }

    fn ifrs(concept: &str, value: f64, period_end: &str, instant: bool) -> RawFact {
        RawFact {
            concept: concept.into(), ctx: String::new(), value: Some(value), text: None, unit: Some("SEK".into()), scale: 0,
            instant: instant.then(|| period_end.to_string()),
            start: (!instant).then(|| format!("{}-01-01", &period_end[..4])),
            end: (!instant).then(|| period_end.to_string()),
            dims: String::new(),
        }
    }

    #[test]
    fn revenue_comes_from_the_best_available_ifrs_concept() {
        use crate::annual_report::merge_raw;
        // SCA: venta de bienes y total de ingresos de explotación -> gana la venta de bienes.
        let sca = vec![ifrs("ifrs-full:RevenueAndOperatingIncome", 23_627e6, "2024-12-31", false), ifrs("ifrs-full:RevenueFromSaleOfGoods", 20_232e6, "2024-12-31", false)];
        assert_eq!(merge_raw(&[sca]).years[0].revenue, Some(20_232_000));
        // Un banco solo trae el total de ingresos de explotación.
        let bank = vec![ifrs("ifrs-full:RevenueAndOperatingIncome", 62_345e6, "2024-12-31", false)];
        assert_eq!(merge_raw(&[bank]).years[0].revenue, Some(62_345_000));
        // Si viene la facturación propiamente dicha, esa manda sea cual sea el orden.
        let both = vec![ifrs("ifrs-full:RevenueFromSaleOfGoods", 1e9, "2024-12-31", false), ifrs("ifrs-full:Revenue", 5e9, "2024-12-31", false)];
        assert_eq!(merge_raw(&[both]).years[0].revenue, Some(5_000_000));
    }

    #[test]
    fn years_without_any_key_figure_are_detected_so_their_own_report_is_read() {
        // El informe 2024 trae 2023 con errores de transformación (sin valor): solo queda una fila vacía.
        let mut r2024 = vec![ifrs("ifrs-full:Assets", 952e9, "2024-12-31", true), ifrs("ifrs-full:Revenue", 63e9, "2024-12-31", false)];
        let mut empty = ifrs("ifrs-full:Assets", 0.0, "2023-12-31", true);
        empty.value = None;
        r2024.push(empty);
        r2024.push(ifrs("ifrs-full:ProfitLossFromOperatingActivities", 1e9, "2023-12-31", false));
        let years = empty_years(&[("2024-12-31".to_string(), r2024)]);
        assert_eq!(years, vec!["2023-12-31".to_string()], "2023 solo tiene el resultado de explotación: sin facturación, resultado ni balance");
    }

    #[test]
    fn opening_balances_dated_on_the_second_of_january_close_the_previous_year() {
        let json = serde_json::json!({ "facts": {
            "a": { "value": "171473000000", "dimensions": { "concept": "ifrs-full:Equity", "entity": "scheme:X", "period": "2021-01-02T00:00:00", "unit": "iso4217:SEK" } },
            "b": { "value": "171473000000", "dimensions": { "concept": "ifrs-full:Equity", "entity": "scheme:X", "period": "2021-01-01T00:00:00", "unit": "iso4217:SEK" } }
        } });
        let facts = parse_xbrl_json(json.to_string().as_bytes()).unwrap();
        assert!(facts.iter().all(|f| f.instant.as_deref() == Some("2020-12-31")), "{:?}", facts.iter().map(|f| &f.instant).collect::<Vec<_>>());
    }

    #[test]
    fn xbrl_json_becomes_the_same_facts_as_ixbrl() {
        let json = serde_json::json!({ "facts": {
            "a": { "value": "292374000000.0", "dimensions": { "concept": "ifrs-full:Assets", "entity": "scheme:X", "period": "2025-01-01T00:00:00", "unit": "iso4217:SEK" } },
            "b": { "value": "247880000000.0", "dimensions": { "concept": "ifrs-full:RevenueFromContractsWithCustomers", "entity": "scheme:X", "period": "2024-01-01T00:00:00/2025-01-01T00:00:00", "unit": "iso4217:SEK" } },
            "c": { "value": "1.5", "dimensions": { "concept": "ifrs-full:Revenue", "entity": "scheme:X", "period": "2024-01-01T00:00:00/2025-01-01T00:00:00", "unit": "iso4217:SEK", "ifrs-full:ProductsAndServicesAxis": "eric:NetworksMember" } },
            "t": { "value": "<div style=\"x\">Grund för <span>rapporten</span>&nbsp;&amp; mer</div>", "dimensions": { "concept": "eric:Nota", "entity": "scheme:X", "period": "2024-01-01T00:00:00/2025-01-01T00:00:00", "language": "sv" } }
        } });
        let facts = parse_xbrl_json(json.to_string().as_bytes()).unwrap();
        let by = |c: &str| facts.iter().find(|f| f.concept == c).unwrap();
        let assets = by("ifrs-full:Assets");
        assert_eq!((assets.instant.as_deref(), assets.value, assets.unit.as_deref()), (Some("2024-12-31"), Some(292_374_000_000.0), Some("SEK")));
        let rev = by("ifrs-full:RevenueFromContractsWithCustomers");
        assert_eq!((rev.start.as_deref(), rev.end.as_deref(), rev.dims.as_str()), (Some("2024-01-01"), Some("2024-12-31"), ""));
        assert!(rev.is_full_year());
        assert_eq!(by("ifrs-full:Revenue").dims, "ifrs-full:ProductsAndServicesAxis=eric:NetworksMember");
        let text = by("eric:Nota");
        assert_eq!((text.value, text.text.as_deref()), (None, Some("Grund för rapporten & mer")));
    }

    #[test]
    fn reports_in_other_currencies_or_empty_are_rejected() {
        let eur = serde_json::json!({ "facts": {
            "a": { "value": "10", "dimensions": { "concept": "ifrs-full:Assets", "entity": "scheme:X", "period": "2025-01-01T00:00:00", "unit": "iso4217:EUR" } },
            "b": { "value": "20", "dimensions": { "concept": "ifrs-full:Equity", "entity": "scheme:X", "period": "2025-01-01T00:00:00", "unit": "iso4217:EUR" } }
        } });
        assert!(parse_xbrl_json(eur.to_string().as_bytes()).unwrap_err().contains("coronas"));
        assert!(parse_xbrl_json(b"{\"facts\":{}}").is_err());
        assert!(parse_xbrl_json(b"no es json").is_err());
    }

    #[test]
    fn leis_are_validated_before_building_urls() {
        assert!(valid_lei("549300W9JLPW15XIFM52"));
        assert!(!valid_lei("549300W9JLPW15XIFM5/"));
        assert!(!valid_lei("../../etc"));
    }
}
