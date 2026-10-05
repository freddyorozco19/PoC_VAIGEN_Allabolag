//! Cifras financieras reales desde las cuentas anuales digitales de Bolagsverket (iXBRL dentro de un ZIP).
//!
//! Qué se ha comprobado contra informes REALES de producción (K2/K3 de varias pymes, 2018-2025):
//! - El ZIP contiene un único `.xhtml` (iXBRL, XML bien formado) con etiquetas `se-gen-base:*`.
//! - Cada informe trae 4 años de resultado (cuadro plurianual) y 2 años de balance.
//! - Las cifras vienen en **coronas** (`scale="0"`) en las cuentas, y otra vez en **miles** (`scale="3"`,
//!   redondeadas) en el cuadro plurianual: se conserva siempre la más precisa (menor `scale`).
//! - El signo va en el atributo `sign="-"`; formatos `numspacecomma`, `numcomma`, `zerodash`.
//! - El ejercicio no siempre es el año natural (p.ej. cierre el 31 de agosto).
//! - Empresas inactivas pueden no traer `Nettoomsattning`: eso es "sin dato", no 0.
//!
//! Para cubrir 5 años de balance se leen hasta 3 informes alternos (años N, N-2 y N-4).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::bolagsverket::{self, BvError, DocRef};

const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_XHTML_BYTES: u64 = 25 * 1024 * 1024;
const MAX_YEARS: usize = 5;

#[derive(Clone, Debug, PartialEq)]
pub struct FinancialYear {
    /// Fin del ejercicio, "AAAA-MM-DD".
    pub period_end: String,
    /// Año de cierre, "2025".
    pub label: String,
    /// Todas en miles de coronas (tkr). `None` = el informe no lo trae.
    pub revenue: Option<i64>,
    pub result: Option<i64>,
    pub equity: Option<i64>,
    pub assets: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Financials {
    /// Del más antiguo al más reciente, máximo 5.
    pub years: Vec<FinancialYear>,
}

impl Financials {
    pub fn latest(&self) -> Option<&FinancialYear> {
        self.years.last()
    }
    pub fn previous(&self) -> Option<&FinancialYear> {
        self.years.len().checked_sub(2).and_then(|i| self.years.get(i))
    }
}

impl FinancialYear {
    /// Soliditet = eget kapital / summa tillgångar, en %.
    pub fn solidity(&self) -> Option<f64> {
        match (self.equity, self.assets) {
            (Some(e), Some(a)) if a > 0 => Some(e as f64 / a as f64 * 100.0),
            _ => None,
        }
    }
    /// Vinstmarginal = resultat efter finansiella poster / omsättning, en %.
    pub fn margin(&self) -> Option<f64> {
        match (self.result, self.revenue) {
            (Some(r), Some(v)) if v > 0 => Some(r as f64 / v as f64 * 100.0),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Metric {
    Revenue,
    Result,
    Equity,
    Assets,
}

fn metric_for(local_name: &str) -> Option<Metric> {
    match local_name {
        "Nettoomsattning" => Some(Metric::Revenue),
        "ResultatEfterFinansiellaPoster" => Some(Metric::Result),
        "EgetKapital" => Some(Metric::Equity),
        "Tillgangar" => Some(Metric::Assets),
        _ => None,
    }
}

/// Un hecho numérico de un informe, ya en coronas.
#[derive(Clone, Debug, PartialEq)]
pub struct Fact {
    pub metric: Metric,
    /// Fin del periodo: fecha del instante (balance) o `endDate` de un ejercicio de ~12 meses (resultado).
    pub period_end: String,
    pub sek: f64,
    /// `scale` del hecho; menor = más preciso (0 = coronas exactas, 3 = miles redondeados).
    pub scale: i32,
}

// ───────────── ZIP → XHTML ─────────────

/// Extrae el informe (`.xhtml`/`.html`/`.xml`) de un ZIP, con límite de tamaño descomprimido.
pub fn extract_xhtml(zip_bytes: &[u8]) -> Result<String, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).map_err(|e| format!("ZIP no válido: {e}"))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("ZIP no válido: {e}"))?;
        let name = entry.name().to_lowercase();
        if entry.is_dir() || !(name.ends_with(".xhtml") || name.ends_with(".html") || name.ends_with(".htm") || name.ends_with(".xml")) {
            continue;
        }
        if entry.size() > MAX_XHTML_BYTES {
            return Err("el informe descomprimido es demasiado grande".to_string());
        }
        let mut text = String::new();
        entry.by_ref().take(MAX_XHTML_BYTES).read_to_string(&mut text).map_err(|e| format!("no se pudo leer el informe: {e}"))?;
        return Ok(text);
    }
    Err("el ZIP no contiene ningún informe iXBRL".to_string())
}

// ───────────── Fechas ─────────────

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn parse_date(s: &str) -> Option<(String, i64)> {
    let s = s.trim().get(..10)?;
    let mut it = s.split('-');
    let (y, m, d): (i64, i64, i64) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then(|| (s.to_string(), days_from_civil(y, m, d)))
}

// ───────────── Números iXBRL ─────────────

/// Interpreta el texto de un `ix:nonFraction` según su `format` (transformaciones `ixt:*`).
/// Devuelve el valor sin aplicar `scale` ni `sign`.
pub fn parse_number(text: &str, format: &str) -> Option<f64> {
    let fmt = format.rsplit(':').next().unwrap_or("").to_lowercase();
    if fmt.contains("zerodash") || fmt.contains("fixed-zero") || fmt.contains("fixedzero") {
        return Some(0.0);
    }
    // Quita espacios de cualquier tipo (incluidos NBSP y espacios finos) y todo lo que no sea número.
    let cleaned: String = text.chars().filter(|c| c.is_ascii_digit() || *c == ',' || *c == '.').collect();
    if cleaned.is_empty() {
        return None;
    }
    let decimal_comma = matches!(fmt.as_str(), "numcomma" | "numspacecomma" | "numdotcomma" | "numcommadecimal");
    let decimal_dot = matches!(fmt.as_str(), "numcommadot" | "numspacedot" | "numdotdecimal");
    let normalised = if decimal_comma {
        cleaned.replace('.', "").replace(',', ".")
    } else if decimal_dot {
        cleaned.replace(',', "")
    } else if cleaned.matches(',').count() == 1 && !cleaned.contains('.') {
        cleaned.replace(',', ".")
    } else {
        cleaned.replace(',', "")
    };
    normalised.parse::<f64>().ok()
}

// ───────────── iXBRL → hechos ─────────────

struct Context {
    /// Fin del periodo si es un instante, o un ejercicio de ~12 meses; `None` en otro caso.
    period_end: Option<String>,
    dimensional: bool,
}

fn element_text(node: roxmltree::Node) -> String {
    node.descendants().filter(|n| n.is_text()).filter_map(|n| n.text()).collect::<String>()
}

fn child_text<'a>(node: roxmltree::Node<'a, 'a>, name: &str) -> Option<String> {
    node.descendants().find(|n| n.is_element() && n.tag_name().name() == name).map(|n| element_text(n).trim().to_string())
}

/// Lee un informe iXBRL y devuelve los hechos de interés (sin deduplicar).
pub fn parse_report(xhtml: &str) -> Result<Vec<Fact>, String> {
    let doc = roxmltree::Document::parse(xhtml).map_err(|e| format!("el informe no es XML válido: {e}"))?;

    let mut contexts: HashMap<String, Context> = HashMap::new();
    for node in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "context") {
        let Some(id) = node.attribute("id") else { continue };
        let dimensional = node.descendants().any(|n| n.is_element() && matches!(n.tag_name().name(), "scenario" | "segment"));
        let period_end = if let Some(instant) = child_text(node, "instant") {
            parse_date(&instant).map(|(d, _)| d)
        } else {
            match (child_text(node, "startDate").and_then(|s| parse_date(&s)), child_text(node, "endDate").and_then(|s| parse_date(&s))) {
                // Solo ejercicios de ~12 meses (no trimestres ni ejercicios largos o cortos).
                (Some((_, a)), Some((end, b))) if (300..=400).contains(&(b - a + 1)) => Some(end),
                _ => None,
            }
        };
        contexts.insert(id.to_string(), Context { period_end, dimensional });
    }
    if contexts.is_empty() {
        return Err("el informe no contiene contextos XBRL".to_string());
    }

    let mut facts = Vec::new();
    for node in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "nonFraction") {
        let Some(name) = node.attribute("name") else { continue };
        let Some(metric) = metric_for(name.rsplit(':').next().unwrap_or(name)) else { continue };
        let Some(ctx) = node.attribute("contextRef").and_then(|c| contexts.get(c)) else { continue };
        if ctx.dimensional {
            continue;
        }
        let Some(period_end) = ctx.period_end.clone() else { continue };
        let Some(raw) = parse_number(&element_text(node), node.attribute("format").unwrap_or("")) else { continue };
        let scale: i32 = node.attribute("scale").and_then(|s| s.trim().parse().ok()).unwrap_or(0);
        let mut sek = raw * 10f64.powi(scale);
        if node.attribute("sign") == Some("-") {
            sek = -sek;
        }
        facts.push(Fact { metric, period_end, sek, scale });
    }
    Ok(facts)
}

// ───────────── Unir informes ─────────────

/// `reports`: los hechos de cada informe, del MÁS RECIENTE al más antiguo.
/// Para cada (métrica, fin de periodo) gana el hecho más preciso (menor `scale`) y, a igualdad, el del informe más reciente.
pub fn merge(reports: &[Vec<Fact>]) -> Financials {
    let mut best: BTreeMap<(Metric, String), (i32, usize, f64)> = BTreeMap::new();
    for (rank, facts) in reports.iter().enumerate() {
        for f in facts {
            let key = (f.metric, f.period_end.clone());
            let candidate = (f.scale, rank, f.sek);
            match best.get(&key) {
                Some(current) if (current.0, current.1) <= (candidate.0, candidate.1) => {}
                _ => {
                    best.insert(key, candidate);
                }
            }
        }
    }
    let ends: BTreeSet<&String> = best.keys().map(|(_, end)| end).collect();
    let mut years: Vec<FinancialYear> = ends
        .into_iter()
        .map(|end| {
            let tkr = |m: Metric| best.get(&(m, end.clone())).map(|(_, _, sek)| (sek / 1000.0).round() as i64);
            FinancialYear {
                period_end: end.clone(),
                label: end.get(..4).unwrap_or(end).to_string(),
                revenue: tkr(Metric::Revenue),
                result: tkr(Metric::Result),
                equity: tkr(Metric::Equity),
                assets: tkr(Metric::Assets),
            }
        })
        .collect();
    if years.len() > MAX_YEARS {
        years.drain(..years.len() - MAX_YEARS);
    }
    Financials { years }
}

/// De la lista de documentos elige los informes a leer. Se ordenan del más reciente al más antiguo y sin
/// duplicados de periodo (queda el registrado más tarde). Cada informe aporta el balance de su ejercicio y el
/// anterior, así que para cubrir los 5 últimos ejercicios (posiciones 0..=4) basta con los informes en las
/// posiciones 0, 2 y 4; si falta el informe de una posición se usa el del anterior (que también la cubre).
/// Máximo 3 descargas.
pub fn pick_reports(mut docs: Vec<DocRef>) -> Vec<DocRef> {
    docs.sort_by(|a, b| b.period_end.cmp(&a.period_end).then(b.registered.cmp(&a.registered)));
    docs.dedup_by(|b, a| a.period_end == b.period_end);

    let mut picks: Vec<usize> = Vec::new();
    let mut covered: isize = -1; // última posición de balance cubierta
    while covered < (MAX_YEARS as isize - 1) && picks.len() < 3 {
        let next = (covered + 1) as usize;
        if next < docs.len() {
            picks.push(next);
            covered = next as isize + 1;
        } else if next >= 1 && next - 1 < docs.len() && !picks.contains(&(next - 1)) {
            picks.push(next - 1);
            covered = next as isize;
        } else {
            break;
        }
    }
    picks.into_iter().map(|i| docs[i].clone()).collect()
}

// ───────────── Consulta con caché ─────────────

type FinCache = HashMap<String, (Instant, Option<Financials>)>;
static CACHE: LazyLock<Mutex<FinCache>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Mira la caché sin llamar a Bolagsverket: `Some(resultado)` si ya hay uno fresco (`Some(None)` = la
/// organización no presenta cuentas digitales); `None` si habría que descargar (la ficha muestra un esqueleto).
pub fn peek(orgnr: &str) -> Option<Option<Financials>> {
    let id = bolagsverket::normalize_org_number(orgnr)?;
    let guard = CACHE.lock().unwrap();
    let (at, value) = guard.get(&id)?;
    (at.elapsed() < CACHE_TTL).then(|| value.clone())
}

/// Cifras de los últimos años de una organización. `Ok(None)` si no tiene cuentas anuales digitales.
pub async fn get_financials(orgnr: &str) -> Result<Option<Financials>, BvError> {
    let id = bolagsverket::normalize_org_number(orgnr)
        .ok_or_else(|| BvError::Invalid(format!("\"{orgnr}\" no es un organisationsnummer válido.")))?;
    if let Some(hit) = peek(&id) {
        return Ok(hit);
    }

    let docs = pick_reports(bolagsverket::list_documents(&id).await?);
    let mut reports: Vec<Vec<Fact>> = Vec::new();
    for (i, doc) in docs.iter().enumerate() {
        let read = async {
            let zip = bolagsverket::download_document(&doc.id).await?;
            let xhtml = extract_xhtml(&zip).map_err(|e| BvError::Upstream { status: None, message: format!("Cuentas anuales {}: {e}", doc.period_end) })?;
            parse_report(&xhtml).map_err(|e| BvError::Upstream { status: None, message: format!("Cuentas anuales {}: {e}", doc.period_end) })
        };
        match read.await {
            Ok(facts) => reports.push(facts),
            // El más reciente es imprescindible; si falla un informe antiguo, se muestra lo que haya.
            Err(e) if i > 0 => eprintln!("{e}"),
            Err(e) => return Err(e),
        }
    }

    let result = if reports.is_empty() { None } else { Some(merge(&reports)) };
    let result = result.filter(|f| !f.years.is_empty());
    CACHE.lock().unwrap().insert(id, (Instant::now(), result.clone()));
    Ok(result)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// iXBRL sintético con la estructura observada en informes reales (no contiene datos de ninguna empresa).
    pub(crate) fn report_xml(end_year: i32, base: i64) -> String {
        let (y1, y0) = (end_year, end_year - 1);
        let rev = |y: i32| base + y as i64; // coronas
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:ix="http://www.xbrl.org/2013/inlineXBRL"
      xmlns:ixt="http://www.xbrl.org/inlineXBRL/transformation/2010-04-20" xmlns:xbrli="http://www.xbrl.org/2003/instance"
      xmlns:se-gen-base="http://www.taxonomier.se/se/fr/gen-base/2021-10-31" xmlns:xbrldi="http://xbrl.org/2006/xbrldi">
<head><title>Årsredovisning</title></head><body>
<div style="display:none"><ix:header><ix:resources>
  <xbrli:context id="d{y1}"><xbrli:entity><xbrli:identifier scheme="x">5560000001</xbrli:identifier></xbrli:entity><xbrli:period><xbrli:startDate>{y1}-01-01</xbrli:startDate><xbrli:endDate>{y1}-12-31</xbrli:endDate></xbrli:period></xbrli:context>
  <xbrli:context id="d{y0}"><xbrli:entity><xbrli:identifier scheme="x">5560000001</xbrli:identifier></xbrli:entity><xbrli:period><xbrli:startDate>{y0}-01-01</xbrli:startDate><xbrli:endDate>{y0}-12-31</xbrli:endDate></xbrli:period></xbrli:context>
  <xbrli:context id="b{y1}"><xbrli:entity><xbrli:identifier scheme="x">5560000001</xbrli:identifier></xbrli:entity><xbrli:period><xbrli:instant>{y1}-12-31</xbrli:instant></xbrli:period></xbrli:context>
  <xbrli:context id="b{y0}"><xbrli:entity><xbrli:identifier scheme="x">5560000001</xbrli:identifier></xbrli:entity><xbrli:period><xbrli:instant>{y0}-12-31</xbrli:instant></xbrli:period></xbrli:context>
  <xbrli:context id="short"><xbrli:entity><xbrli:identifier scheme="x">5560000001</xbrli:identifier></xbrli:entity><xbrli:period><xbrli:startDate>{y1}-07-01</xbrli:startDate><xbrli:endDate>{y1}-12-31</xbrli:endDate></xbrli:period></xbrli:context>
  <xbrli:context id="dim"><xbrli:entity><xbrli:identifier scheme="x">5560000001</xbrli:identifier><xbrli:segment><xbrldi:explicitMember dimension="a:b">c:d</xbrldi:explicitMember></xbrli:segment></xbrli:entity><xbrli:period><xbrli:startDate>{y1}-01-01</xbrli:startDate><xbrli:endDate>{y1}-12-31</xbrli:endDate></xbrli:period></xbrli:context>
</ix:resources></ix:header></div>
<table>
<tr><td>Nettoomsättning</td>
  <td><ix:nonFraction name="se-gen-base:Nettoomsattning" contextRef="d{y1}" unitRef="SEK" decimals="0" scale="0" format="ixt:numspacecomma">{r1}</ix:nonFraction></td>
  <td><ix:nonFraction name="se-gen-base:Nettoomsattning" contextRef="d{y0}" unitRef="SEK" decimals="0" scale="0" format="ixt:numspacecomma">{r0}</ix:nonFraction></td></tr>
<tr><td>Flerårsöversikt (tkr, avrundat)</td>
  <td><ix:nonFraction name="se-gen-base:Nettoomsattning" contextRef="d{y1}" unitRef="SEK" decimals="-3" scale="3" format="ixt:numspacecomma">{r1k}</ix:nonFraction></td></tr>
<tr><td>Resultat efter finansiella poster</td>
  <td>(<ix:nonFraction name="se-gen-base:ResultatEfterFinansiellaPoster" contextRef="d{y1}" unitRef="SEK" decimals="0" scale="0" sign="-" format="ixt:numspacecomma">45&#160;678</ix:nonFraction>)</td>
  <td><ix:nonFraction name="se-gen-base:ResultatEfterFinansiellaPoster" contextRef="d{y0}" unitRef="SEK" scale="0" format="ixt:zerodash">–</ix:nonFraction></td></tr>
<tr><td>Eget kapital</td>
  <td><ix:nonFraction name="se-gen-base:EgetKapital" contextRef="b{y1}" unitRef="SEK" scale="0" format="ixt:numspacecomma">300 000</ix:nonFraction></td>
  <td><ix:nonFraction name="se-gen-base:EgetKapital" contextRef="b{y0}" unitRef="SEK" scale="0" format="ixt:numspacecomma">345 678</ix:nonFraction></td></tr>
<tr><td>Summa tillgångar</td>
  <td><ix:nonFraction name="se-gen-base:Tillgangar" contextRef="b{y1}" unitRef="SEK" scale="0" format="ixt:numspacecomma">900 000</ix:nonFraction></td>
  <td><ix:nonFraction name="se-gen-base:Tillgangar" contextRef="b{y0}" unitRef="SEK" scale="0" format="ixt:numspacecomma">800 000</ix:nonFraction></td></tr>
<tr><td>Ignorados: dimensional y trimestre</td>
  <td><ix:nonFraction name="se-gen-base:Nettoomsattning" contextRef="dim" unitRef="SEK" scale="0" format="ixt:numspacecomma">999</ix:nonFraction></td>
  <td><ix:nonFraction name="se-gen-base:Nettoomsattning" contextRef="short" unitRef="SEK" scale="0" format="ixt:numspacecomma">5</ix:nonFraction></td></tr>
</table></body></html>"#,
            r1 = spaced(rev(y1)),
            r0 = spaced(rev(y0)),
            // Deliberadamente distinto del exacto (+7): si se eligiera por error el hecho redondeado, el test lo vería.
            r1k = rev(y1) / 1000 + 7,
        )
    }

    fn spaced(n: i64) -> String {
        let s = n.to_string();
        let mut out = String::new();
        for (i, c) in s.chars().enumerate() {
            if i > 0 && (s.len() - i) % 3 == 0 {
                out.push(' ');
            }
            out.push(c);
        }
        out
    }

    fn year<'a>(f: &'a Financials, label: &str) -> &'a FinancialYear {
        f.years.iter().find(|y| y.label == label).unwrap_or_else(|| panic!("falta el año {label}"))
    }

    #[test]
    fn parses_numbers_by_ixt_format() {
        assert_eq!(parse_number("1 234 567", "ixt:numspacecomma"), Some(1_234_567.0));
        assert_eq!(parse_number("1\u{00A0}234,56", "ixt:numspacecomma"), Some(1234.56));
        assert_eq!(parse_number("83,5", "ixt:numcomma"), Some(83.5));
        assert_eq!(parse_number("1.234,5", "ixt:numdotcomma"), Some(1234.5));
        assert_eq!(parse_number("1,234.5", "ixt:numcommadot"), Some(1234.5));
        assert_eq!(parse_number("–", "ixt:zerodash"), Some(0.0));
        assert_eq!(parse_number("", "ixt:numspacecomma"), None);
        assert_eq!(parse_number("abc", "ixt:numspacecomma"), None);
    }

    #[test]
    fn parses_a_report_ignoring_dimensional_and_short_periods() {
        let facts = parse_report(&report_xml(2025, 1_000_000)).unwrap();
        let revenue: Vec<_> = facts.iter().filter(|f| f.metric == Metric::Revenue).collect();
        assert!(!revenue.iter().any(|f| f.sek == 999.0 || f.sek == 5.0), "contexto dimensional o trimestral colado");
        let result_2025 = facts.iter().find(|f| f.metric == Metric::Result && f.period_end == "2025-12-31").unwrap();
        assert_eq!(result_2025.sek, -45_678.0, "sign=\"-\" y espacio duro dentro del número");
        let result_2024 = facts.iter().find(|f| f.metric == Metric::Result && f.period_end == "2024-12-31").unwrap();
        assert_eq!(result_2024.sek, 0.0, "zerodash");
    }

    #[test]
    fn merge_prefers_the_most_precise_fact_and_converts_to_tkr() {
        let f = merge(&[parse_report(&report_xml(2025, 1_000_000)).unwrap()]);
        let y = year(&f, "2025");
        // Hay dos hechos de Nettoomsattning para 2025: coronas exactas (scale 0) y miles redondeados (scale 3).
        assert_eq!(y.revenue, Some(1_002), "1 002 025 coronas → 1 002 tkr; gana el exacto (scale 0), no el de miles (1 009)");
        assert_eq!(y.result, Some(-46), "-45 678 coronas → -46 tkr");
        assert_eq!(y.equity, Some(300));
        assert_eq!(y.assets, Some(900));
        assert_eq!(year(&f, "2024").result, Some(0));
        assert_eq!(year(&f, "2024").equity, Some(346));
        assert!((y.solidity().unwrap() - 33.33).abs() < 0.01);
    }

    #[test]
    fn merge_joins_reports_and_keeps_five_years() {
        // Informes de 2025, 2023 y 2021 (cada uno aporta dos años) → 2020..2025 = 6 años; quedan los 5 últimos.
        let reports: Vec<_> = [2025, 2023, 2021].iter().map(|y| parse_report(&report_xml(*y, 1_000_000)).unwrap()).collect();
        let f = merge(&reports);
        assert_eq!(f.years.iter().map(|y| y.label.as_str()).collect::<Vec<_>>(), ["2021", "2022", "2023", "2024", "2025"]);
        assert_eq!(f.latest().unwrap().label, "2025");
        assert_eq!(f.previous().unwrap().label, "2024");
    }

    #[test]
    fn a_newer_report_wins_when_precision_is_equal() {
        let newer = vec![Fact { metric: Metric::Equity, period_end: "2024-12-31".into(), sek: 200_000.0, scale: 0 }];
        let older = vec![Fact { metric: Metric::Equity, period_end: "2024-12-31".into(), sek: 100_000.0, scale: 0 }];
        assert_eq!(merge(&[newer, older]).years[0].equity, Some(200));
    }

    #[test]
    fn missing_revenue_is_none_not_zero() {
        let facts = vec![Fact { metric: Metric::Assets, period_end: "2025-08-31".into(), sek: 0.0, scale: 0 }];
        let y = &merge(&[facts]).years[0];
        assert_eq!(y.revenue, None);
        assert_eq!(y.assets, Some(0));
        assert_eq!(y.label, "2025", "ejercicio que cierra el 31 de agosto");
        assert_eq!(y.margin(), None);
    }

    #[test]
    fn picks_alternate_reports_without_duplicate_periods() {
        let d = |id: &str, end: &str, reg: &str| DocRef { id: id.into(), period_end: end.into(), registered: reg.into() };
        let docs = vec![
            d("a", "2025-12-31", "2026-07-01"),
            d("b", "2024-12-31", "2025-07-01"),
            d("b2", "2024-12-31", "2025-09-01"), // complemento del mismo ejercicio: manda el registrado más tarde
            d("c", "2023-12-31", "2024-07-01"),
            d("d", "2022-12-31", "2023-07-01"),
            d("e", "2021-12-31", "2022-07-01"),
            d("f", "2020-12-31", "2021-07-01"),
        ];
        let picked: Vec<_> = pick_reports(docs).into_iter().map(|d| d.id).collect();
        assert_eq!(picked, ["a", "c", "e"]);
    }

    #[test]
    fn picks_enough_reports_to_cover_balance_years_with_short_histories() {
        let d = |id: &str, end: &str| DocRef { id: id.into(), period_end: end.into(), registered: String::new() };
        let ids = |docs: Vec<DocRef>| pick_reports(docs).into_iter().map(|d| d.id).collect::<Vec<_>>();
        assert!(ids(vec![]).is_empty());
        // Solo uno: cubre su ejercicio y el anterior.
        assert_eq!(ids(vec![d("a", "2025-12-31")]), ["a"]);
        // Dos: el segundo aporta el balance del ejercicio anterior al segundo (que el primero no trae).
        assert_eq!(ids(vec![d("a", "2025-12-31"), d("b", "2024-12-31")]), ["a", "b"]);
        // Tres: 0 y 2 cubren 4 ejercicios de balance; no hay más informes.
        assert_eq!(ids(vec![d("a", "2025-12-31"), d("b", "2024-12-31"), d("c", "2023-12-31")]), ["a", "c"]);
        // Cuatro: 0 y 2 cubren 0..=3 y falta la posición 4 → el informe de la posición 3 la cubre.
        let four = vec![d("a", "2025-12-31"), d("b", "2024-12-31"), d("c", "2023-12-31"), d("e", "2022-12-31")];
        assert_eq!(ids(four), ["a", "c", "e"]);
        // Nunca más de tres descargas.
        let many: Vec<_> = (0..12).map(|i| d(&format!("r{i}"), &format!("{}-12-31", 2025 - i))).collect();
        assert_eq!(ids(many).len(), 3);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_report("no es xml").is_err());
        assert!(parse_report("<html xmlns=\"http://www.w3.org/1999/xhtml\"></html>").is_err(), "sin contextos XBRL");
        assert!(extract_xhtml(b"no es un zip").is_err());
    }

    /// Contra informes REALES ya descargados: `SIFFRA_SAMPLE_REPORTS=<carpeta con subcarpetas x_*> cargo test -- --ignored`.
    #[test]
    #[ignore = "requiere muestras reales en disco"]
    fn real_reports_parse() {
        let dir = std::env::var("SIFFRA_SAMPLE_REPORTS").expect("define SIFFRA_SAMPLE_REPORTS");
        let mut parsed = 0;
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            if !entry.file_name().to_string_lossy().starts_with("x_") {
                continue;
            }
            let file = std::fs::read_dir(entry.path()).unwrap().flatten().find(|f| f.file_name().to_string_lossy().ends_with(".xhtml")).unwrap();
            let facts = parse_report(&std::fs::read_to_string(file.path()).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", entry.path().display()));
            let f = merge(&[facts]);
            println!("{} → {:?}", entry.file_name().to_string_lossy(), f.years.iter().map(|y| (&y.label, y.revenue, y.result, y.equity, y.assets)).collect::<Vec<_>>());
            assert!(!f.years.is_empty() && f.years.iter().any(|y| y.assets.is_some() || y.equity.is_some()), "sin cifras en {}", entry.path().display());
            parsed += 1;
        }
        assert!(parsed >= 1, "no había muestras");
    }
}
