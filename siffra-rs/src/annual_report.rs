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
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::bolagsverket::{self, BvError, DocRef};
use crate::db::Db;

const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_XHTML_BYTES: u64 = 25 * 1024 * 1024;
const MAX_YEARS: usize = 5;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct FinancialYear {
    /// Fin del ejercicio, "AAAA-MM-DD".
    pub period_end: String,
    /// Año de cierre, "2025".
    pub label: String,
    /// Importes en miles de coronas (tkr). `None` = el informe no lo trae.
    pub revenue: Option<i64>,
    /// Resultado después de partidas financieras (el que usan los márgenes de la valoración).
    pub result: Option<i64>,
    pub equity: Option<i64>,
    pub assets: Option<i64>,

    // Cuenta de resultados
    /// Ingresos de explotación totales (incluye otros ingresos y variación de existencias).
    pub operating_income: Option<i64>,
    pub operating_costs: Option<i64>,
    pub personnel_cost: Option<i64>,
    pub other_external: Option<i64>,
    /// Materias primas y mercaderías vendidas (suma de las dos partidas si vienen separadas).
    pub goods_cost: Option<i64>,
    pub depreciation: Option<i64>,
    pub operating_result: Option<i64>,
    /// Resultado financiero neto (ingresos menos gastos financieros).
    pub financial_net: Option<i64>,
    pub interest_expense: Option<i64>,
    pub result_before_tax: Option<i64>,
    pub tax: Option<i64>,
    pub net_result: Option<i64>,

    // Balance
    pub fixed_assets: Option<i64>,
    pub current_assets: Option<i64>,
    pub inventory: Option<i64>,
    pub trade_receivables: Option<i64>,
    pub short_receivables: Option<i64>,
    pub cash: Option<i64>,
    pub share_capital: Option<i64>,
    pub restricted_equity: Option<i64>,
    pub free_equity: Option<i64>,
    pub untaxed_reserves: Option<i64>,
    pub long_term_debt: Option<i64>,
    /// Pasivo corriente (kortfristiga skulder).
    pub short_term_debt: Option<i64>,
    pub trade_payables: Option<i64>,

    /// Plantilla media.
    pub employees: Option<f64>,
    /// Soliditet que declara la propia empresa en su cuadro plurianual, en % (sirve cuando faltan activos o patrimonio).
    pub reported_solidity: Option<f64>,
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

fn ratio(num: Option<i64>, den: Option<i64>) -> Option<f64> {
    match (num, den) {
        (Some(n), Some(d)) if d > 0 => Some(n as f64 / d as f64 * 100.0),
        _ => None,
    }
}

impl FinancialYear {
    /// Soliditet = eget kapital / summa tillgångar, en %. Si el informe no trae las dos cifras se usa la que
    /// declara la propia empresa en su cuadro plurianual.
    pub fn solidity(&self) -> Option<f64> {
        match (self.equity, self.assets) {
            (Some(e), Some(a)) if a > 0 => Some(e as f64 / a as f64 * 100.0),
            _ => self.reported_solidity,
        }
    }
    /// Vinstmarginal = resultat efter finansiella poster / omsättning, en %.
    pub fn margin(&self) -> Option<f64> {
        ratio(self.result, self.revenue)
    }
    /// Margen operativo = resultado de explotación / facturación, en %.
    pub fn operating_margin(&self) -> Option<f64> {
        ratio(self.operating_result, self.revenue)
    }
    /// Margen neto = resultado del ejercicio / facturación, en %.
    pub fn net_margin(&self) -> Option<f64> {
        ratio(self.net_result, self.revenue)
    }
    /// Kassalikviditet = (activo corriente − existencias) / pasivo corriente, en %.
    pub fn liquidity(&self) -> Option<f64> {
        let quick = self.current_assets? - self.inventory.unwrap_or(0);
        ratio(Some(quick), self.short_term_debt)
    }
    /// Razón corriente = activo corriente / pasivo corriente, en %.
    pub fn current_ratio(&self) -> Option<f64> {
        ratio(self.current_assets, self.short_term_debt)
    }
    /// Caja / pasivo corriente, en %.
    pub fn cash_ratio(&self) -> Option<f64> {
        ratio(self.cash, self.short_term_debt)
    }
    /// Capital circulante = activo corriente − pasivo corriente (tkr).
    pub fn working_capital(&self) -> Option<i64> {
        Some(self.current_assets? - self.short_term_debt?)
    }
    /// Deudas totales = activos − patrimonio neto (incluye reservas y provisiones), en tkr.
    pub fn total_liabilities(&self) -> Option<i64> {
        Some(self.assets? - self.equity?)
    }
    /// Skuldsättningsgrad = deudas totales / patrimonio neto (veces). `None` con patrimonio no positivo.
    pub fn debt_to_equity(&self) -> Option<f64> {
        match (self.total_liabilities(), self.equity) {
            (Some(d), Some(e)) if e > 0 => Some(d as f64 / e as f64),
            _ => None,
        }
    }
    /// Rentabilidad sobre el patrimonio neto (ROE), en %.
    pub fn roe(&self) -> Option<f64> {
        ratio(self.net_result, self.equity)
    }
    /// Rentabilidad sobre los activos (ROA) = (resultado antes de impuestos + gastos financieros) / activos, en %.
    pub fn roa(&self) -> Option<f64> {
        ratio(Some(self.result_before_tax? + self.interest_expense.unwrap_or(0)), self.assets)
    }
    /// Rotación de activos = facturación / activos (veces).
    pub fn asset_turnover(&self) -> Option<f64> {
        match (self.revenue, self.assets) {
            (Some(r), Some(a)) if a > 0 => Some(r as f64 / a as f64),
            _ => None,
        }
    }
    /// Gastos de personal / facturación, en %.
    pub fn personnel_share(&self) -> Option<f64> {
        ratio(self.personnel_cost, self.revenue)
    }
    /// Facturación por empleado (tkr).
    pub fn revenue_per_employee(&self) -> Option<i64> {
        match (self.revenue, self.employees) {
            (Some(r), Some(e)) if e > 0.0 => Some((r as f64 / e).round() as i64),
            _ => None,
        }
    }
    /// Cobertura de intereses = (resultado antes de impuestos + gastos financieros) / gastos financieros (veces).
    pub fn interest_cover(&self) -> Option<f64> {
        let interest = self.interest_expense.filter(|i| *i > 0)?;
        Some((self.result_before_tax? + interest) as f64 / interest as f64)
    }
    /// Patrimonio neto / capital social, en %: por debajo del 50 % la ley sueca obliga a una "kontrollbalansräkning".
    pub fn equity_to_share_capital(&self) -> Option<f64> {
        ratio(self.equity, self.share_capital)
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

// ───────────── iXBRL → todos los hechos ─────────────

/// Un hecho de un informe tal como viene, sin descartar nada: cifras (con o sin desglose por dimensión) y textos.
#[derive(Clone, Debug, PartialEq)]
pub struct RawFact {
    /// Concepto con su prefijo, p. ej. `se-gen-base:Nettoomsattning`.
    pub concept: String,
    /// Identificador del contexto del informe: los hechos de una misma persona o partida comparten contexto.
    pub ctx: String,
    /// Cifra (en la unidad del hecho, ya con `scale` y `sign` aplicados) o `None` si es un texto.
    pub value: Option<f64>,
    /// Texto del hecho si no es numérico (acotado) y, en los numéricos, el texto tal como se escribió en el informe.
    pub text: Option<String>,
    /// Unidad (`SEK`, `procent`, `antal`…), si la trae.
    pub unit: Option<String>,
    /// `scale` del hecho; menor = más preciso (0 = coronas exactas, 3 = miles redondeados).
    pub scale: i32,
    /// Instante (balance) o `None` si es un periodo.
    pub instant: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    /// Desglose: `Eje=Miembro;Eje=Miembro` ordenado, vacío si el hecho es del total de la empresa.
    pub dims: String,
}

impl RawFact {
    /// Nombre del concepto sin prefijo.
    pub fn local(&self) -> &str {
        self.concept.rsplit(':').next().unwrap_or(&self.concept)
    }
    /// Fin del periodo: el instante, o la fecha final de un periodo.
    pub fn period_end(&self) -> Option<&str> {
        self.instant.as_deref().or(self.end.as_deref())
    }
    /// ¿Es un periodo de ~12 meses (ejercicio completo)?
    pub fn is_full_year(&self) -> bool {
        match (&self.start, &self.end) {
            (Some(s), Some(e)) => match (parse_date(s), parse_date(e)) {
                (Some((_, a)), Some((_, b))) => (300..=400).contains(&(b - a + 1)),
                _ => false,
            },
            _ => false,
        }
    }
}

/// Máximo de texto que se guarda por hecho no numérico (los informes traen la memoria entera como texto).
const MAX_FACT_TEXT: usize = 20_000;

struct RawContext {
    instant: Option<String>,
    start: Option<String>,
    end: Option<String>,
    dims: String,
}

fn context_dims(node: roxmltree::Node) -> String {
    let mut parts: Vec<String> = Vec::new();
    for n in node.descendants().filter(|n| n.is_element()) {
        match n.tag_name().name() {
            "explicitMember" => {
                let dim = n.attribute("dimension").unwrap_or("?");
                parts.push(format!("{dim}={}", element_text(n).trim()));
            }
            "typedMember" => {
                let dim = n.attribute("dimension").unwrap_or("?");
                parts.push(format!("{dim}={}", element_text(n).trim()));
            }
            _ => {}
        }
    }
    parts.sort();
    parts.join(";")
}

/// Lee un informe iXBRL y devuelve TODOS sus hechos (cifras y textos, con y sin desglose), sin deduplicar.
pub fn parse_all(xhtml: &str) -> Result<Vec<RawFact>, String> {
    let doc = roxmltree::Document::parse(xhtml).map_err(|e| format!("el informe no es XML válido: {e}"))?;

    let mut contexts: HashMap<String, RawContext> = HashMap::new();
    for node in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "context") {
        let Some(id) = node.attribute("id") else { continue };
        let date = |name: &str| child_text(node, name).and_then(|s| parse_date(&s)).map(|(d, _)| d);
        contexts.insert(id.to_string(), RawContext { instant: date("instant"), start: date("startDate"), end: date("endDate"), dims: context_dims(node) });
    }
    if contexts.is_empty() {
        return Err("el informe no contiene contextos XBRL".to_string());
    }

    let mut facts = Vec::new();
    for node in doc.descendants().filter(|n| n.is_element() && matches!(n.tag_name().name(), "nonFraction" | "nonNumeric")) {
        let Some(name) = node.attribute("name") else { continue };
        let Some(ctx) = node.attribute("contextRef").and_then(|c| contexts.get(c)) else { continue };
        let numeric = node.tag_name().name() == "nonFraction";
        let raw_text = element_text(node);
        let (value, scale) = if numeric {
            let scale: i32 = node.attribute("scale").and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            // Un hecho vacío (xsi:nil o sin texto) no tiene cifra: se conserva como hecho sin valor.
            let value = parse_number(&raw_text, node.attribute("format").unwrap_or("")).map(|raw| {
                let v = raw * 10f64.powi(scale);
                if node.attribute("sign") == Some("-") { -v } else { v }
            });
            (value, scale)
        } else {
            (None, 0)
        };
        let text = raw_text.trim();
        facts.push(RawFact {
            ctx: node.attribute("contextRef").unwrap_or("").to_string(),
            concept: name.to_string(),
            value,
            text: (!text.is_empty()).then(|| text.chars().take(MAX_FACT_TEXT).collect()),
            unit: node.attribute("unitRef").map(str::to_string),
            scale,
            instant: ctx.instant.clone(),
            start: ctx.start.clone(),
            end: ctx.end.clone(),
            dims: ctx.dims.clone(),
        });
    }
    Ok(facts)
}

/// Los hechos de interés del resumen (4 cifras del total de la empresa), a partir de todos los del informe.
pub fn core_facts(all: &[RawFact]) -> Vec<Fact> {
    all.iter()
        .filter(|f| f.dims.is_empty())
        .filter_map(|f| {
            let metric = metric_for(f.local())?;
            let sek = f.value?;
            // Balance (instante) o ejercicio de ~12 meses; no trimestres ni ejercicios largos o cortos.
            let period_end = if f.instant.is_some() { f.instant.clone() } else if f.is_full_year() { f.end.clone() } else { None }?;
            Some(Fact { metric, period_end, sek, scale: f.scale })
        })
        .collect()
}

/// Lee un informe iXBRL y devuelve los hechos de interés (sin deduplicar).
pub fn parse_report(xhtml: &str) -> Result<Vec<Fact>, String> {
    Ok(core_facts(&parse_all(xhtml)?))
}

fn element_text(node: roxmltree::Node) -> String {
    node.descendants().filter(|n| n.is_text()).filter_map(|n| n.text()).collect::<String>()
}

fn child_text<'a>(node: roxmltree::Node<'a, 'a>, name: &str) -> Option<String> {
    node.descendants().find(|n| n.is_element() && n.tag_name().name() == name).map(|n| element_text(n).trim().to_string())
}

// ───────────── Unir informes ─────────────

/// Campo del resumen de un año al que contribuye un concepto del informe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Field {
    Revenue,
    OperatingIncome,
    OperatingCosts,
    PersonnelCost,
    OtherExternal,
    Materials,
    GoodsForResale,
    Depreciation,
    OperatingResult,
    FinancialNet,
    InterestExpense,
    ResultAfterFinancial,
    ResultBeforeTax,
    Tax,
    NetResult,
    FixedAssets,
    CurrentAssets,
    Inventory,
    TradeReceivables,
    ShortReceivables,
    Cash,
    Assets,
    Equity,
    ShareCapital,
    RestrictedEquity,
    FreeEquity,
    UntaxedReserves,
    LongTermDebt,
    ShortTermDebt,
    TradePayables,
    Employees,
    ReportedSolidity,
}

/// Concepto de la taxonomía sueca (`se-gen-base`) → campo. Nombres comprobados contra informes reales K2/K3.
fn field_for(local: &str) -> Option<Field> {
    Some(match local {
        "Nettoomsattning" => Field::Revenue,
        "RorelseintakterLagerforandringarMm" => Field::OperatingIncome,
        "Rorelsekostnader" => Field::OperatingCosts,
        "Personalkostnader" => Field::PersonnelCost,
        "OvrigaExternaKostnader" => Field::OtherExternal,
        "RavarorFornodenheterKostnader" => Field::Materials,
        "HandelsvarorKostnader" => Field::GoodsForResale,
        "AvskrivningarNedskrivningarMateriellaImmateriellaAnlaggningstillgangar" => Field::Depreciation,
        "Rorelseresultat" => Field::OperatingResult,
        "FinansiellaPoster" => Field::FinancialNet,
        "RantekostnaderLiknandeResultatposter" => Field::InterestExpense,
        "ResultatEfterFinansiellaPoster" => Field::ResultAfterFinancial,
        "ResultatForeSkatt" => Field::ResultBeforeTax,
        "SkattAretsResultat" => Field::Tax,
        "AretsResultat" => Field::NetResult,
        "Anlaggningstillgangar" => Field::FixedAssets,
        "Omsattningstillgangar" => Field::CurrentAssets,
        "VarulagerMm" => Field::Inventory,
        "Kundfordringar" => Field::TradeReceivables,
        "KortfristigaFordringar" => Field::ShortReceivables,
        "KassaBank" => Field::Cash,
        "Tillgangar" => Field::Assets,
        "EgetKapital" => Field::Equity,
        "Aktiekapital" => Field::ShareCapital,
        "BundetEgetKapital" => Field::RestrictedEquity,
        "FrittEgetKapital" => Field::FreeEquity,
        "ObeskattadeReserver" => Field::UntaxedReserves,
        "LangfristigaSkulder" => Field::LongTermDebt,
        "KortfristigaSkulder" => Field::ShortTermDebt,
        "Leverantorsskulder" => Field::TradePayables,
        "MedelantaletAnstallda" => Field::Employees,
        "Soliditet" => Field::ReportedSolidity,
        _ => return None,
    })
}

/// `reports`: los hechos de cada informe, del MÁS RECIENTE al más antiguo.
/// Para cada (campo, fin de periodo) gana el hecho más preciso (menor `scale`) y, a igualdad, el del informe más reciente.
/// Solo cuentan los hechos del total de la empresa (sin desglose) de un instante o de un ejercicio de ~12 meses.
pub fn merge_raw(reports: &[Vec<RawFact>]) -> Financials {
    let mut best: BTreeMap<(Field, String), (i32, usize, f64)> = BTreeMap::new();
    for (rank, facts) in reports.iter().enumerate() {
        for f in facts {
            if !f.dims.is_empty() {
                continue;
            }
            let (Some(field), Some(value)) = (field_for(f.local()), f.value) else { continue };
            let Some(period_end) = (if f.instant.is_some() { f.instant.clone() } else if f.is_full_year() { f.end.clone() } else { None }) else { continue };
            let key = (field, period_end);
            let candidate = (f.scale, rank, value);
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
            let raw = |f: Field| best.get(&(f, end.clone())).map(|(_, _, v)| *v);
            let tkr = |f: Field| raw(f).map(|sek| (sek / 1000.0).round() as i64);
            let goods = match (tkr(Field::Materials), tkr(Field::GoodsForResale)) {
                (None, None) => None,
                (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
            };
            FinancialYear {
                period_end: end.clone(),
                label: end.get(..4).unwrap_or(end).to_string(),
                revenue: tkr(Field::Revenue),
                result: tkr(Field::ResultAfterFinancial),
                equity: tkr(Field::Equity),
                assets: tkr(Field::Assets),
                operating_income: tkr(Field::OperatingIncome),
                operating_costs: tkr(Field::OperatingCosts),
                personnel_cost: tkr(Field::PersonnelCost),
                other_external: tkr(Field::OtherExternal),
                goods_cost: goods,
                depreciation: tkr(Field::Depreciation),
                operating_result: tkr(Field::OperatingResult),
                financial_net: tkr(Field::FinancialNet),
                interest_expense: tkr(Field::InterestExpense),
                result_before_tax: tkr(Field::ResultBeforeTax),
                tax: tkr(Field::Tax),
                net_result: tkr(Field::NetResult),
                fixed_assets: tkr(Field::FixedAssets),
                current_assets: tkr(Field::CurrentAssets),
                inventory: tkr(Field::Inventory),
                trade_receivables: tkr(Field::TradeReceivables),
                short_receivables: tkr(Field::ShortReceivables),
                cash: tkr(Field::Cash),
                share_capital: tkr(Field::ShareCapital),
                restricted_equity: tkr(Field::RestrictedEquity),
                free_equity: tkr(Field::FreeEquity),
                untaxed_reserves: tkr(Field::UntaxedReserves),
                long_term_debt: tkr(Field::LongTermDebt),
                short_term_debt: tkr(Field::ShortTermDebt),
                trade_payables: tkr(Field::TradePayables),
                employees: raw(Field::Employees),
                // Se declara como fracción (0,168 = 16,8 %).
                reported_solidity: raw(Field::ReportedSolidity).map(|v| if v.abs() <= 1.5 { v * 100.0 } else { v }),
            }
        })
        .collect();
    if years.len() > MAX_YEARS {
        years.drain(..years.len() - MAX_YEARS);
    }
    Financials { years }
}

/// Une los hechos de interés (4 cifras) de varios informes; solo se usa en las pruebas.
#[cfg(test)]
pub fn merge(reports: &[Vec<Fact>]) -> Financials {
    let raw: Vec<Vec<RawFact>> = reports
        .iter()
        .map(|facts| {
            facts
                .iter()
                .map(|f| RawFact {
                    ctx: String::new(),
                    concept: format!(
                        "se-gen-base:{}",
                        match f.metric {
                            Metric::Revenue => "Nettoomsattning",
                            Metric::Result => "ResultatEfterFinansiellaPoster",
                            Metric::Equity => "EgetKapital",
                            Metric::Assets => "Tillgangar",
                        }
                    ),
                    value: Some(f.sek),
                    text: None,
                    unit: Some("SEK".into()),
                    scale: f.scale,
                    instant: Some(f.period_end.clone()),
                    start: None,
                    end: None,
                    dims: String::new(),
                })
                .collect()
        })
        .collect();
    merge_raw(&raw)
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

// ───────────── Almacén de informes ─────────────

/// Dónde se guardan los informes: todos sus hechos en la base de datos y, si hay carpeta, el documento original
/// (ZIP) tal como lo entrega Bolagsverket. Nada se descarta: si mejora el análisis, se vuelve a leer lo guardado
/// sin pedir otra vez los documentos (el límite es de 60 consultas por minuto).
#[derive(Clone)]
pub struct Store {
    pub db: Db,
    pub dir: Option<PathBuf>,
}

static STORE: RwLock<Option<Store>> = RwLock::new(None);

/// Activa el almacén para todo el proceso (se llama una vez al arrancar).
pub fn set_store(store: Store) {
    *STORE.write().unwrap_or_else(|e| e.into_inner()) = Some(store);
}

fn current_store() -> Option<Store> {
    STORE.read().unwrap_or_else(|e| e.into_inner()).clone()
}

impl Store {
    /// Guarda el documento original y todos sus hechos.
    fn save(&self, orgnr: &str, doc: &DocRef, zip: &[u8], facts: &[RawFact]) {
        let raw_path = self.dir.as_ref().and_then(|dir| {
            // Los identificadores ya están validados (letras, cifras, `_` y `-`): no pueden salir de la carpeta.
            if !bolagsverket::valid_document_id(&doc.id) || !orgnr.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let folder = dir.join(orgnr);
            std::fs::create_dir_all(&folder).ok()?;
            let path = folder.join(format!("{}.zip", doc.id));
            std::fs::write(&path, zip).ok()?;
            Some(path.to_string_lossy().into_owned())
        });
        self.db.report_save(orgnr, &doc.id, &doc.period_end, &doc.registered, raw_path.as_deref(), facts);
    }
}

/// Informes ya guardados de una empresa, con todos sus hechos (el de ejercicio más reciente primero).
pub fn stored_reports(orgnr: &str) -> Vec<(crate::db::ReportInfo, Vec<RawFact>)> {
    let Some(store) = current_store() else { return vec![] };
    store.db.reports_of(orgnr).into_iter().filter_map(|info| store.db.report_facts(&info.doc_id).map(|facts| (info, facts))).collect()
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

/// Lee un informe: de la base de datos si ya se guardó, o lo descarga, lo guarda entero y lo devuelve.
async fn load_report(store: Option<&Store>, orgnr: &str, doc: &DocRef) -> Result<Vec<RawFact>, BvError> {
    if let Some(facts) = store.and_then(|s| s.db.report_facts(&doc.id)) {
        return Ok(facts);
    }
    let zip = bolagsverket::download_document(&doc.id).await?;
    let fail = |e: String| BvError::Upstream { status: None, message: format!("Cuentas anuales {}: {e}", doc.period_end) };
    let xhtml = extract_xhtml(&zip).map_err(&fail)?;
    let facts = parse_all(&xhtml).map_err(&fail)?;
    if let Some(s) = store {
        s.save(orgnr, doc, &zip, &facts);
    }
    Ok(facts)
}

/// Cifras de los últimos años de una organización. `Ok(None)` si no tiene cuentas anuales digitales.
pub async fn get_financials(orgnr: &str) -> Result<Option<Financials>, BvError> {
    let id = bolagsverket::normalize_org_number(orgnr)
        .ok_or_else(|| BvError::Invalid(format!("\"{orgnr}\" no es un organisationsnummer válido.")))?;
    if let Some(hit) = peek(&id) {
        return Ok(hit);
    }

    let store = current_store();
    let docs = pick_reports(bolagsverket::list_documents(&id).await?);
    let mut reports: Vec<Vec<RawFact>> = Vec::new();
    for (i, doc) in docs.iter().enumerate() {
        match load_report(store.as_ref(), &id, doc).await {
            Ok(facts) => reports.push(facts),
            // El más reciente es imprescindible; si falla un informe antiguo, se muestra lo que haya.
            Err(e) if i > 0 => eprintln!("{e}"),
            Err(e) => return Err(e),
        }
    }

    let result = if reports.is_empty() { None } else { Some(merge_raw(&reports)) };
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

/// Pruebas del análisis ampliado: se leen y se guardan TODOS los hechos del informe y de ahí salen los ratios.
#[cfg(test)]
pub(crate) mod rich_tests {
    use super::*;

    fn nf(name: &str, ctx: &str, value: &str, extra: &str) -> String {
        format!(r#"<ix:nonFraction name="se-gen-base:{name}" contextRef="{ctx}" unitRef="SEK" decimals="0" scale="0" format="ixt:numspacecomma"{extra}>{value}</ix:nonFraction>"#)
    }

    /// Informe sintético con la mayoría de los conceptos de un informe real (K2/K3), un hecho con desglose, textos y
    /// dos firmantes. Los importes están pensados para que los ratios den cifras redondas.
    pub(crate) fn rich_report() -> String {
        let facts = [
            // Resultado 2025 y 2024
            nf("Nettoomsattning", "d25", "1 000 000", ""),
            nf("Nettoomsattning", "d24", "800 000", ""),
            nf("RorelseintakterLagerforandringarMm", "d25", "1 010 000", ""),
            nf("Rorelsekostnader", "d25", "930 000", ""),
            nf("Personalkostnader", "d25", "400 000", ""),
            nf("OvrigaExternaKostnader", "d25", "300 000", ""),
            nf("RavarorFornodenheterKostnader", "d25", "150 000", ""),
            nf("HandelsvarorKostnader", "d25", "50 000", ""),
            nf("AvskrivningarNedskrivningarMateriellaImmateriellaAnlaggningstillgangar", "d25", "20 000", ""),
            nf("Rorelseresultat", "d25", "80 000", ""),
            nf("FinansiellaPoster", "d25", "10 000", r#" sign="-""#),
            nf("RantekostnaderLiknandeResultatposter", "d25", "12 000", ""),
            nf("ResultatEfterFinansiellaPoster", "d25", "70 000", ""),
            nf("ResultatForeSkatt", "d25", "70 000", ""),
            nf("SkattAretsResultat", "d25", "14 000", r#" sign="-""#),
            nf("AretsResultat", "d25", "56 000", ""),
            nf("ResultatEfterFinansiellaPoster", "d24", "40 000", ""),
            nf("AretsResultat", "d24", "32 000", ""),
            // Balance 2025 y 2024
            nf("Tillgangar", "b25", "900 000", ""),
            nf("Anlaggningstillgangar", "b25", "300 000", ""),
            nf("Omsattningstillgangar", "b25", "600 000", ""),
            nf("VarulagerMm", "b25", "100 000", ""),
            nf("Kundfordringar", "b25", "150 000", ""),
            nf("KortfristigaFordringar", "b25", "200 000", ""),
            nf("KassaBank", "b25", "250 000", ""),
            nf("EgetKapital", "b25", "450 000", ""),
            nf("Aktiekapital", "b25", "100 000", ""),
            nf("BundetEgetKapital", "b25", "100 000", ""),
            nf("FrittEgetKapital", "b25", "350 000", ""),
            nf("LangfristigaSkulder", "b25", "200 000", ""),
            nf("KortfristigaSkulder", "b25", "250 000", ""),
            nf("Leverantorsskulder", "b25", "80 000", ""),
            nf("Tillgangar", "b24", "800 000", ""),
            nf("EgetKapital", "b24", "394 000", ""),
            // Plantilla y soliditet declarada (fracción: 50 con scale -2 = 0,5)
            nf("MedelantaletAnstallda", "d25", "5", ""),
            r#"<ix:nonFraction name="se-gen-base:Soliditet" contextRef="b25" unitRef="procent" decimals="3" scale="-2" format="ixt:numcomma">50</ix:nonFraction>"#.to_string(),
            // Un hecho con desglose por categoría (no entra en el resumen pero se conserva)
            r#"<ix:nonFraction name="se-gen-base:Nettoomsattning" contextRef="seg" unitRef="SEK" decimals="0" scale="0" format="ixt:numspacecomma">600 000</ix:nonFraction>"#.to_string(),
            // Textos y firmantes
            r#"<ix:nonNumeric name="se-gen-base:AllmantVerksamheten" contextRef="d25">Bolaget bedriver konsultverksamhet.</ix:nonNumeric>"#.to_string(),
            r#"<ix:nonNumeric name="se-gen-base:UnderskriftHandlingTilltalsnamn" contextRef="p1">Anna</ix:nonNumeric>"#.to_string(),
            r#"<ix:nonNumeric name="se-gen-base:UnderskriftHandlingEfternamn" contextRef="p1">Test</ix:nonNumeric>"#.to_string(),
            r#"<ix:nonNumeric name="se-gen-base:UnderskriftHandlingRoll" contextRef="p1">Verkställande direktör</ix:nonNumeric>"#.to_string(),
            r#"<ix:nonNumeric name="se-gen-base:UnderskriftHandlingTilltalsnamn" contextRef="p2">Bo</ix:nonNumeric>"#.to_string(),
            r#"<ix:nonNumeric name="se-gen-base:UnderskriftHandlingEfternamn" contextRef="p2">Prov</ix:nonNumeric>"#.to_string(),
            r#"<ix:nonNumeric name="se-gen-base:UnderskriftHandlingRoll" contextRef="p2">Styrelseordförande</ix:nonNumeric>"#.to_string(),
        ]
        .join("\n");
        let ctx = |id: &str, period: &str| format!(r#"<xbrli:context id="{id}"><xbrli:entity><xbrli:identifier scheme="x">5560000001</xbrli:identifier></xbrli:entity><xbrli:period>{period}</xbrli:period></xbrli:context>"#);
        let year = |y: i32| format!("<xbrli:startDate>{y}-01-01</xbrli:startDate><xbrli:endDate>{y}-12-31</xbrli:endDate>");
        let at = |y: i32| format!("<xbrli:instant>{y}-12-31</xbrli:instant>");
        let contexts = [ctx("d25", &year(2025)), ctx("d24", &year(2024)), ctx("b25", &at(2025)), ctx("b24", &at(2024)), ctx("p1", &year(2025)), ctx("p2", &year(2025))].join("\n");
        let seg = format!(
            r#"<xbrli:context id="seg"><xbrli:entity><xbrli:identifier scheme="x">5560000001</xbrli:identifier><xbrli:segment><xbrldi:explicitMember dimension="se-gen-base:SegmentAxel">se-gen-base:ProductoMember</xbrldi:explicitMember></xbrli:segment></xbrli:entity><xbrli:period>{}</xbrli:period></xbrli:context>"#,
            year(2025)
        );
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:ix="http://www.xbrl.org/2013/inlineXBRL" xmlns:ixt="http://www.xbrl.org/inlineXBRL/transformation/2010-04-20"
      xmlns:xbrli="http://www.xbrl.org/2003/instance" xmlns:se-gen-base="http://www.taxonomier.se/se/fr/gen-base/2021-10-31" xmlns:xbrldi="http://xbrl.org/2006/xbrldi">
<head><title>Årsredovisning</title></head><body>
<div style="display:none"><ix:header><ix:resources>
{contexts}
{seg}
</ix:resources></ix:header></div>
<p>{facts}</p></body></html>"#
        )
    }

    #[test]
    fn every_fact_of_the_report_is_kept_numbers_dimensions_texts_and_contexts() {
        let all = parse_all(&rich_report()).unwrap();
        let numeric = all.iter().filter(|f| f.value.is_some()).count();
        let texts = all.iter().filter(|f| f.value.is_none() && f.text.is_some()).count();
        assert_eq!((numeric, texts), (37, 7), "{} hechos", all.len());
        // La cifra con desglose se conserva con su eje y miembro.
        let seg = all.iter().find(|f| !f.dims.is_empty()).unwrap();
        assert_eq!((seg.local(), seg.value, seg.dims.as_str()), ("Nettoomsattning", Some(600_000.0), "se-gen-base:SegmentAxel=se-gen-base:ProductoMember"));
        // Signo y escala aplicados; unidad conservada.
        let fin = all.iter().find(|f| f.local() == "FinansiellaPoster").unwrap();
        assert_eq!((fin.value, fin.unit.as_deref()), (Some(-10_000.0), Some("SEK")));
        let solidity = all.iter().find(|f| f.local() == "Soliditet").unwrap();
        assert!((solidity.value.unwrap() - 0.5).abs() < 1e-12 && solidity.unit.as_deref() == Some("procent"));
        // Los textos y sus contextos (los firmantes comparten contexto).
        let ctxs: Vec<(&str, &str)> = all.iter().filter(|f| f.local().starts_with("UnderskriftHandling")).map(|f| (f.ctx.as_str(), f.text.as_deref().unwrap())).collect();
        assert_eq!(ctxs.iter().filter(|(c, _)| *c == "p1").count(), 3);
        assert!(ctxs.contains(&("p2", "Styrelseordförande")));
        assert_eq!(all.iter().find(|f| f.local() == "AllmantVerksamheten").unwrap().text.as_deref(), Some("Bolaget bedriver konsultverksamhet."));
        // El resumen clásico de 4 cifras sigue saliendo igual (sin el desglose).
        let core = core_facts(&all);
        assert_eq!(core.iter().filter(|f| f.metric == Metric::Revenue).count(), 2, "dos años de facturación; el desglose no cuenta");
    }

    #[test]
    fn merge_fills_the_full_year_summary_and_the_ratios_follow() {
        let f = merge_raw(&[parse_all(&rich_report()).unwrap()]);
        let y = f.latest().unwrap();
        assert_eq!(y.label, "2025");
        assert_eq!((y.revenue, y.operating_income, y.operating_costs), (Some(1_000), Some(1_010), Some(930)));
        assert_eq!((y.personnel_cost, y.other_external, y.goods_cost, y.depreciation), (Some(400), Some(300), Some(200), Some(20)), "materias primas + mercaderías");
        assert_eq!((y.operating_result, y.financial_net, y.interest_expense), (Some(80), Some(-10), Some(12)));
        assert_eq!((y.result, y.result_before_tax, y.tax, y.net_result), (Some(70), Some(70), Some(-14), Some(56)));
        assert_eq!((y.assets, y.fixed_assets, y.current_assets, y.inventory), (Some(900), Some(300), Some(600), Some(100)));
        assert_eq!((y.trade_receivables, y.short_receivables, y.cash), (Some(150), Some(200), Some(250)));
        assert_eq!((y.equity, y.share_capital, y.restricted_equity, y.free_equity), (Some(450), Some(100), Some(100), Some(350)));
        assert_eq!((y.long_term_debt, y.short_term_debt, y.trade_payables), (Some(200), Some(250), Some(80)));
        assert_eq!(y.employees, Some(5.0));
        assert!((y.reported_solidity.unwrap() - 50.0).abs() < 1e-9, "0,5 declarado = 50 %");
        // 2024: solo lo que el informe trae de ese año; el resto queda sin dato, no en cero.
        let p = f.previous().unwrap();
        assert_eq!((p.label.as_str(), p.revenue, p.equity, p.assets, p.cash), ("2024", Some(800), Some(394), Some(800), None));

        // Ratios (todos con resultado redondo).
        assert!((y.solidity().unwrap() - 50.0).abs() < 1e-9);
        assert!((y.margin().unwrap() - 7.0).abs() < 1e-9 && (y.operating_margin().unwrap() - 8.0).abs() < 1e-9 && (y.net_margin().unwrap() - 5.6).abs() < 1e-9);
        assert!((y.liquidity().unwrap() - 200.0).abs() < 1e-9, "(600 − 100) / 250");
        assert!((y.current_ratio().unwrap() - 240.0).abs() < 1e-9 && (y.cash_ratio().unwrap() - 100.0).abs() < 1e-9);
        assert_eq!((y.working_capital(), y.total_liabilities()), (Some(350), Some(450)));
        assert!((y.debt_to_equity().unwrap() - 1.0).abs() < 1e-9);
        assert!((y.roe().unwrap() - 56.0 / 450.0 * 100.0).abs() < 1e-9);
        assert!((y.roa().unwrap() - 82.0 / 900.0 * 100.0).abs() < 1e-9, "(70 + 12) / 900");
        assert!((y.asset_turnover().unwrap() - 1000.0 / 900.0).abs() < 1e-9);
        assert!((y.personnel_share().unwrap() - 40.0).abs() < 1e-9);
        assert_eq!(y.revenue_per_employee(), Some(200));
        assert!((y.interest_cover().unwrap() - 82.0 / 12.0).abs() < 1e-9);
        assert!((y.equity_to_share_capital().unwrap() - 450.0).abs() < 1e-9);
    }

    #[test]
    fn ratios_are_none_not_zero_when_the_inputs_are_missing_or_meaningless() {
        let y = FinancialYear { equity: Some(-100), assets: Some(500), revenue: Some(0), employees: Some(0.0), interest_expense: Some(0), ..Default::default() };
        assert_eq!((y.debt_to_equity(), y.roe(), y.margin(), y.operating_margin(), y.revenue_per_employee(), y.interest_cover(), y.liquidity()), (None, None, None, None, None, None, None));
        assert!((y.solidity().unwrap() + 20.0).abs() < 1e-9, "patrimonio negativo: solidez negativa");
        // Sin activos ni patrimonio, vale la soliditet que declara el informe.
        let only_reported = FinancialYear { reported_solidity: Some(22.5), ..Default::default() };
        assert_eq!(only_reported.solidity(), Some(22.5));
    }

    #[test]
    fn the_store_keeps_everything_and_a_saved_report_is_never_downloaded_again() {
        let db = Db::memory();
        let facts = parse_all(&rich_report()).unwrap();
        db.report_save("5560000001", "doc-1_paket", "2025-12-31", "2026-03-01", Some("/x/doc.zip"), &facts);
        let back = db.report_facts("doc-1_paket").unwrap();
        assert_eq!(back, facts, "ida y vuelta sin perder ni un hecho, con sus contextos y desgloses");
        assert!(db.report_facts("otro").is_none(), "un documento que no se guardó se pide a Bolagsverket");
        let infos = db.reports_of("5560000001");
        assert_eq!((infos.len(), infos[0].fact_count, infos[0].raw_path.as_deref()), (1, facts.len() as i64, Some("/x/doc.zip")));
        // Guardar otra vez el mismo documento lo reemplaza, no lo duplica.
        db.report_save("5560000001", "doc-1_paket", "2025-12-31", "2026-03-01", None, &facts[..3]);
        assert_eq!(db.report_facts("doc-1_paket").unwrap().len(), 3);
        assert_eq!(db.reports_of("5560000001").len(), 1);
        // El resumen sale igual de lo guardado que de lo recién leído.
        assert_eq!(merge_raw(&[back]), merge_raw(&[facts]));
    }

    #[test]
    fn the_raw_document_goes_to_disk_under_the_organisation_and_ids_cannot_escape() {
        let dir = std::env::temp_dir().join(format!("siffra-reports-{}", crate::util::random_hex(4)));
        let store = Store { db: Db::memory(), dir: Some(dir.clone()) };
        let doc = DocRef { id: "abc-123_paket".into(), period_end: "2025-12-31".into(), registered: "2026-03-01".into() };
        store.save("5560000001", &doc, b"zip-bytes", &[]);
        let info = &store.db.reports_of("5560000001")[0];
        assert_eq!(std::fs::read(info.raw_path.as_ref().unwrap()).unwrap(), b"zip-bytes");
        // Un identificador con `..` o un número que no es de cifras no escribe fuera de la carpeta.
        let evil = DocRef { id: "../../fuera".into(), period_end: "2025-12-31".into(), registered: String::new() };
        store.save("5560000001", &evil, b"x", &[]);
        store.save("../x", &doc, b"x", &[]);
        assert!(!dir.parent().unwrap().join("fuera.zip").exists());
        assert_eq!(store.db.reports_of("../x")[0].raw_path, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
