//! Valoración financiera de una empresa real a partir de sus cuentas anuales (Bolagsverket), la mediana de su
//! sector (SCB) y su estado en el registro. Son reglas simples y explicables, no un modelo: cada señal dice qué
//! se midió y con qué cifras, y el nivel de riesgo es la peor de las señales.
//!
//! Qué NO se puede valorar con los datos que hay: liquidez (las cuentas leídas no traen activo ni pasivo
//! corriente), plantilla (la API no da empleados) ni el capital social. Cuando faltan cuentas digitales la
//! empresa queda "sin valorar" en vez de recibir una nota inventada.

use crate::annual_report::{FinancialYear, Financials};
use crate::bolagsverket::Organisation;
use crate::model::Severity;
use crate::scb::SectorMedians;

/// Meses de retraso a partir de los cuales el último informe presentado se considera desactualizado.
const STALE_MONTHS: i32 = 18;
/// Solidez por debajo de la cual hay poco colchón de capital propio, en %.
const LOW_SOLIDITY: f64 = 10.0;
/// Una cifra por debajo de la mediana solo es señal de alerta si además es débil por sí misma: una pyme con
/// 28 % de solidez frente a una mediana del 68 % no está en apuros (la mediana la arrastran las sociedades
/// holding y los vehículos con poca deuda). Los gráficos siguen mostrando la comparación sin veredicto.
const WEAK_SOLIDITY: f64 = 20.0;
const WEAK_MARGIN: f64 = 2.0;
/// Kassalikviditet: por debajo de 100 % hay que vigilar; por debajo de 50 % es crítico.
const LOW_LIQUIDITY: f64 = 100.0;
const CRITICAL_LIQUIDITY: f64 = 50.0;
/// Veces que el resultado antes de gastos financieros cubre los intereses.
const WEAK_INTEREST_COVER: f64 = 1.5;
/// Diferencia (en puntos) con la mediana del sector para hablar de "por encima" o "por debajo".
const SOLIDITY_BAND: f64 = 5.0;
const MARGIN_BAND: f64 = 2.0;
/// Variación de facturación (en %) que se considera relevante.
const REVENUE_MOVE: f64 = 10.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Signal {
    /// Procedimiento en curso según el registro (texto original en sueco: "Konkurs (sedan …)").
    Insolvency(String),
    /// Fecha de baja.
    Deregistered(String),
    NoFilings,
    /// Fin del ejercicio del último informe presentado.
    StaleReport(String),
    NegativeEquity(i64),
    /// Resultado del ejercicio anterior y del último (tkr).
    LossStreak(i64, i64),
    Loss(i64),
    LowSolidity(f64),
    /// Liquidez (kassalikviditet) por debajo del 100 %: no cubre sus deudas a corto plazo sin vender existencias.
    LowLiquidity(f64),
    /// Liquidez por debajo del 50 %.
    CriticalLiquidity(f64),
    /// Patrimonio neto, capital social: el primero cayó por debajo de la mitad del segundo (kontrollbalansräkning).
    CapitalLost(i64, i64),
    /// Cobertura de intereses por debajo de 1,5 veces.
    WeakInterestCover(f64),
    /// (valor de la empresa, mediana del sector)
    SolidityBelowMedian(f64, f64),
    SolidityAboveMedian(f64, f64),
    MarginBelowMedian(f64, f64),
    MarginAboveMedian(f64, f64),
    /// (variación en %, año anterior, último año); la caída va en positivo.
    RevenueDrop(f64, String, String),
    RevenueGrowth(f64, String, String),
    /// Ejercicios seguidos con beneficio (mínimo 3).
    Profitable(usize),
}

impl Signal {
    pub fn severity(&self) -> Severity {
        match self {
            Signal::Insolvency(_) | Signal::NegativeEquity(_) | Signal::LossStreak(..) | Signal::LowSolidity(_) | Signal::CriticalLiquidity(_) | Signal::CapitalLost(..) => Severity::Bad,
            Signal::Deregistered(_)
            | Signal::NoFilings
            | Signal::StaleReport(_)
            | Signal::Loss(_)
            | Signal::LowLiquidity(_)
            | Signal::WeakInterestCover(_)
            | Signal::SolidityBelowMedian(..)
            | Signal::MarginBelowMedian(..)
            | Signal::RevenueDrop(..) => Severity::Warn,
            Signal::SolidityAboveMedian(..) | Signal::MarginAboveMedian(..) | Signal::RevenueGrowth(..) | Signal::Profitable(_) => Severity::Good,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Assessment {
    /// `None` = sin valorar (no hay cuentas digitales con cifras suficientes).
    pub level: Option<Severity>,
    /// De la más grave a la más favorable.
    pub signals: Vec<Signal>,
}

fn rank(s: Severity) -> u8 {
    match s {
        Severity::Bad => 0,
        Severity::Warn => 1,
        Severity::Good => 2,
    }
}

/// Meses entre dos fechas "AAAA-MM-DD" (positivo si `to` es posterior a `from`).
fn months_between(from: &str, to: &str) -> Option<i32> {
    let ym = |d: &str| -> Option<i32> { Some(d.get(0..4)?.parse::<i32>().ok()? * 12 + d.get(5..7)?.parse::<i32>().ok()?) };
    Some(ym(to)? - ym(from)?)
}

fn consecutive_profits(years: &[FinancialYear]) -> usize {
    years.iter().rev().take_while(|y| y.result.is_some_and(|r| r > 0)).count()
}

/// Valora la empresa. `today` es "AAAA-MM-DD" (se pasa de fuera para poder probarlo).
pub fn assess(org: &Organisation, fin: Option<&Financials>, medians: Option<&SectorMedians>, today: &str) -> Assessment {
    let mut signals: Vec<Signal> = Vec::new();

    for p in &org.forfaranden {
        signals.push(Signal::Insolvency(p.clone()));
    }
    if let Some(d) = &org.avregistreringsdatum {
        signals.push(Signal::Deregistered(d.clone()));
    }

    // Sin cuentas con cifras no hay valoración financiera (pero un procedimiento de insolvencia sí cuenta).
    let usable = fin.filter(|f| f.latest().is_some_and(|y| y.result.is_some() || y.equity.is_some() || y.revenue.is_some()));
    let Some(fin) = usable else {
        signals.push(Signal::NoFilings);
        sort(&mut signals);
        let level = signals.iter().any(|s| s.severity() == Severity::Bad).then_some(Severity::Bad);
        return Assessment { level, signals };
    };
    let latest = fin.latest().expect("comprobado arriba");
    let previous = fin.previous();

    // En un informe ESEF el retraso suele ser del índice público y no de la empresa: no se avisa.
    if !fin.consolidated && months_between(&latest.period_end, today).is_some_and(|m| m > STALE_MONTHS) {
        signals.push(Signal::StaleReport(latest.period_end.clone()));
    }

    if let Some(equity) = latest.equity.filter(|e| *e < 0) {
        signals.push(Signal::NegativeEquity(equity));
    }
    match (latest.result, previous.and_then(|p| p.result)) {
        (Some(last), Some(before)) if last < 0 && before < 0 => signals.push(Signal::LossStreak(before, last)),
        (Some(last), _) if last < 0 => signals.push(Signal::Loss(last)),
        _ => {}
    }

    let solidity = latest.solidity();
    if let Some(s) = solidity.filter(|s| *s >= 0.0 && *s < LOW_SOLIDITY) {
        signals.push(Signal::LowSolidity(s));
    }
    // Capital social perdido: con patrimonio negativo ya salta NegativeEquity, que es más preciso.
    if let (Some(equity), Some(capital)) = (latest.equity, latest.share_capital) {
        if equity >= 0 && capital > 0 && equity * 2 < capital {
            signals.push(Signal::CapitalLost(equity, capital));
        }
    }
    if let Some(l) = latest.liquidity() {
        if l < CRITICAL_LIQUIDITY {
            signals.push(Signal::CriticalLiquidity(l));
        } else if l < LOW_LIQUIDITY {
            signals.push(Signal::LowLiquidity(l));
        }
    }
    if let Some(cover) = latest.interest_cover().filter(|c| *c < WEAK_INTEREST_COVER) {
        signals.push(Signal::WeakInterestCover(cover));
    }
    if let (Some(s), Some(m)) = (solidity, medians) {
        if s < m.solidity - SOLIDITY_BAND && s < WEAK_SOLIDITY {
            signals.push(Signal::SolidityBelowMedian(s, m.solidity));
        } else if s >= m.solidity + SOLIDITY_BAND {
            signals.push(Signal::SolidityAboveMedian(s, m.solidity));
        }
    }
    if let (Some(v), Some(m)) = (latest.margin(), medians) {
        if v < m.margin - MARGIN_BAND && v < WEAK_MARGIN {
            signals.push(Signal::MarginBelowMedian(v, m.margin));
        } else if v >= m.margin + MARGIN_BAND {
            signals.push(Signal::MarginAboveMedian(v, m.margin));
        }
    }

    if let (Some(now), Some(before), Some(prev)) = (latest.revenue, previous.and_then(|p| p.revenue), previous) {
        if before > 0 && now >= 0 {
            let change = (now as f64 / before as f64 - 1.0) * 100.0;
            if change <= -REVENUE_MOVE {
                signals.push(Signal::RevenueDrop(-change, prev.label.clone(), latest.label.clone()));
            } else if change >= REVENUE_MOVE {
                signals.push(Signal::RevenueGrowth(change, prev.label.clone(), latest.label.clone()));
            }
        }
    }

    let streak = consecutive_profits(&fin.years);
    if streak >= 3 {
        signals.push(Signal::Profitable(streak));
    }

    sort(&mut signals);
    let level = signals.iter().map(Signal::severity).min_by_key(|s| rank(*s)).or(Some(Severity::Good));
    Assessment { level, signals }
}

fn sort(signals: &mut [Signal]) {
    signals.sort_by_key(|s| rank(s.severity())); // estable: conserva el orden de cálculo dentro de cada gravedad
}

/// Marcas de estado del registro que se guardan en la foto de una empresa.
pub const FLAG_INSOLVENCY: i64 = 1;
pub const FLAG_DEREGISTERED: i64 = 2;

/// Resumen de una empresa en un momento dado: lo que se guarda en "Mis empresas" para detectar cambios y lo que
/// se muestra al comparar. Todas las cifras en miles de coronas (tkr).
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub level: Option<Severity>,
    /// Año del último ejercicio con cifras.
    pub year: Option<String>,
    pub revenue: Option<i64>,
    pub result: Option<i64>,
    pub equity: Option<i64>,
    pub solidity: Option<f64>,
    pub margin: Option<f64>,
    /// Variación de la facturación respecto al ejercicio anterior, en %.
    pub growth: Option<f64>,
    /// `FLAG_*` combinados.
    pub flags: i64,
}

/// Foto de la empresa con las mismas reglas que la valoración de la ficha.
pub fn snapshot(org: &Organisation, fin: Option<&Financials>, medians: Option<&SectorMedians>, today: &str) -> Snapshot {
    let level = assess(org, fin, medians, today).level;
    let latest = fin.and_then(|f| f.latest());
    let previous = fin.and_then(|f| f.previous());
    let growth = match (latest.and_then(|y| y.revenue), previous.and_then(|y| y.revenue)) {
        (Some(now), Some(before)) if before > 0 => Some((now as f64 / before as f64 - 1.0) * 100.0),
        _ => None,
    };
    let mut flags = 0;
    if !org.forfaranden.is_empty() {
        flags |= FLAG_INSOLVENCY;
    }
    if org.avregistreringsdatum.is_some() {
        flags |= FLAG_DEREGISTERED;
    }
    Snapshot {
        level,
        year: latest.map(|y| y.label.clone()),
        revenue: latest.and_then(|y| y.revenue),
        result: latest.and_then(|y| y.result),
        equity: latest.and_then(|y| y.equity),
        solidity: latest.and_then(|y| y.solidity()),
        margin: latest.and_then(|y| y.margin()),
        growth,
        flags,
    }
}

/// Nivel como texto para guardarlo (`""` = sin valorar) y de vuelta.
pub fn level_code(level: Option<Severity>) -> &'static str {
    match level {
        None => "",
        Some(Severity::Good) => "good",
        Some(Severity::Warn) => "warn",
        Some(Severity::Bad) => "bad",
    }
}

pub fn parse_level(code: &str) -> Option<Severity> {
    match code {
        "good" => Some(Severity::Good),
        "warn" => Some(Severity::Warn),
        "bad" => Some(Severity::Bad),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn org() -> Organisation {
        Organisation {
            organisationsnummer: "5560000019".into(),
            namn: "Prueba AB".into(),
            organisationsform: "Aktiebolag".into(),
            sni: vec![("62010".into(), "Dataprogrammering".into())],
            gatuadress: None,
            postnummer: None,
            postort: None,
            registreringsdatum: "2000-01-01".into(),
            avregistreringsdatum: None,
            aktiv: true,
            forfaranden: vec![],
            verksamhetsbeskrivning: None,
        }
    }

    fn year(label: &str, revenue: i64, result: i64, equity: i64, assets: i64) -> FinancialYear {
        FinancialYear {
            period_end: format!("{label}-12-31"),
            label: label.into(),
            revenue: Some(revenue),
            result: Some(result),
            equity: Some(equity),
            assets: Some(assets),
            ..Default::default()
        }
    }

    fn medians(margin: f64, solidity: f64) -> SectorMedians {
        SectorMedians { margin, solidity, liquidity: 100.0, year: "2024".into(), sni_code: "62.010".into(), sni_label: String::new(), size_class: "TOT".into(), exact_sni: true, exact_size: false }
    }

    fn fin(years: Vec<FinancialYear>) -> Financials {
        Financials { years, consolidated: false }
    }



    #[test]
    fn snapshot_condenses_the_assessment_for_lists_and_comparisons() {
        let f = fin(vec![year("2023", 10_000, 900, 3_000, 8_000), year("2024", 11_000, 1_100, 3_900, 8_500)]);
        let s = snapshot(&org(), Some(&f), None, "2025-06-01");
        assert_eq!(s.level, Some(Severity::Good));
        assert_eq!((s.year.as_deref(), s.revenue, s.result, s.equity), (Some("2024"), Some(11_000), Some(1_100), Some(3_900)));
        assert!((s.growth.unwrap() - 10.0).abs() < 1e-9 && (s.margin.unwrap() - 10.0).abs() < 1e-9);
        assert!((s.solidity.unwrap() - 45.88).abs() < 0.01);
        assert_eq!(s.flags, 0);

        let mut bad = org();
        bad.forfaranden = vec!["Konkurs (sedan 2024-01-26)".into()];
        bad.avregistreringsdatum = Some("2024-03-01".into());
        let b = snapshot(&bad, None, None, "2025-06-01");
        assert_eq!((b.level, b.year, b.revenue), (Some(Severity::Bad), None, None));
        assert_eq!(b.flags, FLAG_INSOLVENCY | FLAG_DEREGISTERED);
    }

    #[test]
    fn level_codes_round_trip() {
        for l in [None, Some(Severity::Good), Some(Severity::Warn), Some(Severity::Bad)] {
            assert_eq!(parse_level(level_code(l)), l);
        }
        assert_eq!(parse_level("raro"), None);
    }

    #[test]
    fn below_the_sector_median_only_alerts_when_the_figure_is_weak_on_its_own() {
        // 28 % de solidez y 7 % de margen frente a medianas de 68 % y 18 %: lejos de la mediana, pero sana.
        let healthy = fin(vec![year("2023", 2_600, 190, 580, 2_100), year("2024", 2_519, 176, 600, 2_150)]);
        let a = assess(&org(), Some(&healthy), Some(&medians(18.2, 68.0)), "2025-06-01");
        assert!(!a.signals.iter().any(|s| matches!(s, Signal::SolidityBelowMedian(..) | Signal::MarginBelowMedian(..))), "{:?}", a.signals);
        assert_eq!(a.level, Some(Severity::Good));
        // 17 % de solidez y 0,5 % de margen: débil por sí misma y por debajo del sector → vigilar.
        let weak = fin(vec![year("2023", 820, 4, 150, 880), year("2024", 941, 5, 150, 900)]);
        let b = assess(&org(), Some(&weak), Some(&medians(18.5, 67.0)), "2025-06-01");
        assert!(b.signals.iter().any(|s| matches!(s, Signal::SolidityBelowMedian(..))));
        assert!(b.signals.iter().any(|s| matches!(s, Signal::MarginBelowMedian(..))));
        assert_eq!(b.level, Some(Severity::Warn));
    }

    #[test]
    fn healthy_company_is_good_and_explains_why() {
        let f = fin(vec![
            year("2022", 10_000, 900, 3_000, 8_000),
            year("2023", 11_000, 1_000, 3_800, 8_500),
            year("2024", 12_500, 1_300, 4_800, 9_000),
        ]);
        let a = assess(&org(), Some(&f), Some(&medians(5.0, 30.0)), "2025-06-01");
        assert_eq!(a.level, Some(Severity::Good));
        assert!(a.signals.iter().all(|s| s.severity() == Severity::Good), "{:?}", a.signals);
        assert!(a.signals.contains(&Signal::Profitable(3)));
        assert!(a.signals.iter().any(|s| matches!(s, Signal::SolidityAboveMedian(v, m) if (*v - 53.33).abs() < 0.1 && *m == 30.0)));
        assert!(a.signals.iter().any(|s| matches!(s, Signal::RevenueGrowth(g, from, to) if (*g - 13.6).abs() < 0.1 && from == "2023" && to == "2024")));
    }

    #[test]
    fn two_losses_in_a_row_are_high_risk() {
        let f = fin(vec![year("2023", 80_000, -2_100, 9_700, 37_500), year("2024", 71_000, -3_800, 5_300, 35_100)]);
        let a = assess(&org(), Some(&f), Some(&medians(4.4, 28.0)), "2025-06-01");
        assert_eq!(a.level, Some(Severity::Bad));
        assert_eq!(a.signals[0], Signal::LossStreak(-2_100, -3_800), "lo más grave primero");
        assert!(a.signals.iter().any(|s| matches!(s, Signal::LowSolidity(v) if *v > 15.0)) == false, "15,1 % no es 'muy baja'");
        assert!(a.signals.iter().any(|s| matches!(s, Signal::SolidityBelowMedian(..))));
        assert!(a.signals.iter().any(|s| matches!(s, Signal::RevenueDrop(d, ..) if (*d - 11.25).abs() < 0.1)));
    }

    #[test]
    fn a_single_loss_is_a_warning_and_negative_equity_is_high_risk() {
        let one = fin(vec![year("2023", 5_000, 200, 2_000, 6_000), year("2024", 5_100, -150, 1_850, 6_100)]);
        let a = assess(&org(), Some(&one), None, "2025-06-01");
        assert_eq!(a.level, Some(Severity::Warn));
        assert!(a.signals.contains(&Signal::Loss(-150)));
        let neg = fin(vec![year("2024", 5_000, -500, -300, 2_000)]);
        let b = assess(&org(), Some(&neg), None, "2025-06-01");
        assert_eq!(b.level, Some(Severity::Bad));
        assert!(b.signals.contains(&Signal::NegativeEquity(-300)));
        assert!(!b.signals.iter().any(|s| matches!(s, Signal::LowSolidity(_))), "con patrimonio negativo ya hay una señal más precisa");
    }

    #[test]
    fn very_low_solidity_is_high_risk() {
        let f = fin(vec![year("2024", 5_000, 100, 150, 4_000)]);
        let a = assess(&org(), Some(&f), None, "2025-06-01");
        assert_eq!(a.level, Some(Severity::Bad));
        assert!(a.signals.iter().any(|s| matches!(s, Signal::LowSolidity(v) if (*v - 3.75).abs() < 0.01)));
    }

    #[test]
    fn stale_report_is_a_warning_only_after_eighteen_months() {
        let f = fin(vec![year("2023", 5_000, 300, 2_000, 5_000)]);
        assert_eq!(assess(&org(), Some(&f), None, "2025-06-30").level, Some(Severity::Good), "18 meses justos");
        let a = assess(&org(), Some(&f), None, "2025-07-15");
        assert_eq!(a.level, Some(Severity::Warn));
        assert!(a.signals.contains(&Signal::StaleReport("2023-12-31".into())));
    }

    #[test]
    fn without_digital_accounts_the_company_is_not_rated() {
        let a = assess(&org(), None, None, "2025-06-01");
        assert_eq!((a.level, a.signals.clone()), (None, vec![Signal::NoFilings]));
        let empty = fin(vec![FinancialYear { period_end: "2024-12-31".into(), label: "2024".into(), ..Default::default() }]);
        assert_eq!(assess(&org(), Some(&empty), None, "2025-06-01").level, None);
    }

    #[test]
    fn insolvency_proceedings_and_deregistration_come_from_the_register() {
        let mut o = org();
        o.forfaranden = vec!["Konkurs (sedan 2024-01-26)".into()];
        let a = assess(&o, None, None, "2025-06-01");
        assert_eq!(a.level, Some(Severity::Bad), "un concurso es riesgo alto aunque no haya cuentas");
        assert_eq!(a.signals[0], Signal::Insolvency("Konkurs (sedan 2024-01-26)".into()));
        let mut gone = org();
        gone.avregistreringsdatum = Some("2023-05-05".into());
        gone.aktiv = false;
        let b = assess(&gone, None, None, "2025-06-01");
        assert_eq!(b.level, None);
        assert!(b.signals.contains(&Signal::Deregistered("2023-05-05".into())));
    }

    #[test]
    fn small_differences_with_the_sector_are_not_flagged() {
        let f = fin(vec![year("2023", 10_000, 500, 2_900, 10_000), year("2024", 10_300, 520, 3_100, 10_000)]);
        let a = assess(&org(), Some(&f), Some(&medians(5.0, 30.0)), "2025-06-01");
        assert!(a.signals.is_empty(), "31 % vs 30 % y 5,05 % vs 5 % no dicen nada: {:?}", a.signals);
        assert_eq!(a.level, Some(Severity::Good));
    }

    #[test]
    fn detailed_report_data_feeds_liquidity_capital_and_interest_signals() {
        let base = |extra: FinancialYear| FinancialYear { period_end: "2024-12-31".into(), label: "2024".into(), revenue: Some(5_000), result: Some(200), equity: Some(2_000), assets: Some(4_000), ..extra };
        // Liquidez 80 % → vigilar; 40 % → crítica.
        let low = fin(vec![base(FinancialYear { current_assets: Some(200), short_term_debt: Some(250), ..Default::default() })]);
        let a = assess(&org(), Some(&low), None, "2025-06-01");
        assert!(a.signals.iter().any(|s| matches!(s, Signal::LowLiquidity(v) if (*v - 80.0).abs() < 1e-9)));
        assert_eq!(a.level, Some(Severity::Warn));
        let critical = fin(vec![base(FinancialYear { current_assets: Some(300), inventory: Some(200), short_term_debt: Some(250), ..Default::default() })]);
        let b = assess(&org(), Some(&critical), None, "2025-06-01");
        assert!(b.signals.iter().any(|s| matches!(s, Signal::CriticalLiquidity(v) if (*v - 40.0).abs() < 1e-9)), "las existencias no cuentan: (300 − 200) / 250");
        assert_eq!(b.level, Some(Severity::Bad));
        // Una liquidez sana no dice nada.
        let fine = fin(vec![base(FinancialYear { current_assets: Some(900), short_term_debt: Some(250), ..Default::default() })]);
        assert!(assess(&org(), Some(&fine), None, "2025-06-01").signals.iter().all(|s| !matches!(s, Signal::LowLiquidity(_) | Signal::CriticalLiquidity(_))));

        // Capital social perdido: patrimonio 40 con capital social 100 → kontrollbalansräkning.
        let lost = fin(vec![FinancialYear { equity: Some(40), share_capital: Some(100), ..base(Default::default()) }]);
        let c = assess(&org(), Some(&lost), None, "2025-06-01");
        assert!(c.signals.contains(&Signal::CapitalLost(40, 100)) && c.level == Some(Severity::Bad));
        // 60 de 100 no llega a la mitad perdida; y con patrimonio negativo manda NegativeEquity.
        let ok = fin(vec![FinancialYear { equity: Some(60), share_capital: Some(100), assets: Some(400), ..base(Default::default()) }]);
        assert!(!assess(&org(), Some(&ok), None, "2025-06-01").signals.iter().any(|s| matches!(s, Signal::CapitalLost(..))));
        let neg = fin(vec![FinancialYear { equity: Some(-5), share_capital: Some(100), ..base(Default::default()) }]);
        assert!(!assess(&org(), Some(&neg), None, "2025-06-01").signals.iter().any(|s| matches!(s, Signal::CapitalLost(..))));

        // Cobertura de intereses: (60 + 50) / 50 = 2,2 veces es suficiente; (10 + 50) / 50 = 1,2 es débil.
        let enough = fin(vec![base(FinancialYear { result_before_tax: Some(60), interest_expense: Some(50), ..Default::default() })]);
        assert!(!assess(&org(), Some(&enough), None, "2025-06-01").signals.iter().any(|s| matches!(s, Signal::WeakInterestCover(_))));
        let weak = fin(vec![base(FinancialYear { result_before_tax: Some(10), interest_expense: Some(50), ..Default::default() })]);
        assert!(assess(&org(), Some(&weak), None, "2025-06-01").signals.iter().any(|s| matches!(s, Signal::WeakInterestCover(v) if (*v - 1.2).abs() < 1e-9)));
    }

    #[test]
    fn months_between_handles_year_boundaries() {
        assert_eq!(months_between("2023-12-31", "2025-06-30"), Some(18));
        assert_eq!(months_between("2024-12-31", "2025-01-01"), Some(1));
        assert_eq!(months_between("basura", "2025-01-01"), None);
    }
}
