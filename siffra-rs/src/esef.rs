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
                None => instant = date_of(period, true),
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
    let from_store = |docs: Vec<DocRef>| -> Vec<Vec<RawFact>> { pick_reports(docs).iter().filter_map(|d| store.and_then(|s| s.db.report_facts(&d.id))).collect() };
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

async fn fetch(store: Option<&Store>, gleif: &str, filings_base: &str, orgnr: &str) -> Result<Vec<Vec<RawFact>>, String> {
    let Some(lei) = lei_for(gleif, orgnr).await? else { return Ok(vec![]) };
    let filings = list_filings(filings_base, &lei).await?;
    let docs: Vec<DocRef> = filings.iter().map(|f| DocRef { id: format!("{DOC_PREFIX}{lei}-{}", f.period_end), period_end: f.period_end.clone(), registered: f.registered.clone() }).collect();
    let picked = pick_reports(docs);

    let mut reports = Vec::new();
    for (i, doc) in picked.iter().enumerate() {
        if let Some(facts) = store.and_then(|s| s.db.report_facts(&doc.id)) {
            reports.push(facts);
            continue;
        }
        // Entre varias versiones del mismo ejercicio (sueco/inglés, reenvíos) gana la registrada más tarde.
        let Some(filing) = filings.iter().filter(|f| f.period_end == doc.period_end).max_by(|a, b| a.registered.cmp(&b.registered)) else { continue };
        let bytes = match download_json(filings_base, &filing.json_url).await {
            Ok(b) => b,
            Err(e) if i > 0 => {
                eprintln!("ESEF {orgnr} {}: {e}", doc.period_end);
                continue;
            }
            Err(e) => return Err(e),
        };
        match parse_xbrl_json(&bytes) {
            Ok(facts) => {
                if let Some(s) = store {
                    s.save_esef(orgnr, doc, &bytes, &facts);
                }
                reports.push(facts);
            }
            // Un informe ilegible o en otra moneda no se muestra, pero tampoco hace fallar la ficha.
            Err(e) => eprintln!("ESEF {orgnr} {}: {e}", doc.period_end),
        }
    }
    Ok(reports)
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
