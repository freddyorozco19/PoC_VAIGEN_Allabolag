//! Modelo de dominio y datos de EJEMPLO de Siffra.
//!
//! Las tres empresas son FICTICIAS. Todos los textos visibles (actividad, roles, señales, estados…) son
//! CLAVES del catálogo de idiomas (`catalog.rs`), no texto: así se muestran en español, inglés o sueco.
//! Los nombres propios (empresas, personas, ciudades, clientes) no se traducen.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Good,
    Warn,
    Bad,
}

/// Mismo conjunto de valores que `AlertSeverity` en el original.
pub type RiskLevel = Severity;

pub struct CompanyAlert {
    pub severity: Severity,
    /// Clave del catálogo.
    pub text: &'static str,
}

pub struct CompanyPerson {
    /// Clave del catálogo (cargo).
    pub role: &'static str,
    pub name: &'static str,
}

/// Cinco años de una misma magnitud, en miles de kronor (tkr), del más antiguo al más reciente.
pub type FiveYearSeries = [i64; 5];

pub struct FinancialHistory {
    /// Omsättning: ingresos netos.
    pub revenue: FiveYearSeries,
    /// Resultat efter finansiella poster.
    pub result: FiveYearSeries,
    /// Eget kapital.
    pub equity: FiveYearSeries,
    /// Summa tillgångar.
    pub total_assets: FiveYearSeries,
}

/// `value` = la propia empresa, `median` = mediana del sector (SNI).
#[derive(Clone, Copy)]
pub struct BenchmarkPair {
    pub value: f64,
    pub median: f64,
}

pub struct CompanyBenchmarks {
    pub margin: BenchmarkPair,
    pub solidity: BenchmarkPair,
    pub liquidity: BenchmarkPair,
}

pub struct Company {
    pub id: &'static str,
    pub name: &'static str,
    /// Organisationsnummer sueco, formato NNNNNN-NNNN.
    pub org_number: &'static str,
    /// Clave: forma jurídica.
    pub legal_form: &'static str,
    /// Código SNI ("52.290") y clave de su descripción.
    pub sni_code: &'static str,
    pub sni_text: &'static str,
    pub city: &'static str,
    pub employee_range: &'static str,
    /// Clave: estado.
    pub status: &'static str,
    pub risk_level: RiskLevel,
    /// Clave: firmateckning.
    pub signing: &'static str,
    pub people: &'static [CompanyPerson],
    pub alerts: &'static [CompanyAlert],
    pub financials: FinancialHistory,
    pub benchmarks: CompanyBenchmarks,
}

/// Los cinco años calendario que cubren las series de `FinancialHistory`.
pub const FINANCIAL_YEARS: [&str; 5] = ["2020", "2021", "2022", "2023", "2024"];

pub struct WatchAlert {
    pub severity: Severity,
    pub company_name: &'static str,
    /// Claves del catálogo.
    pub text: &'static str,
    pub when: &'static str,
}

pub struct CashWeek {
    pub week: u32,
    pub inflow: i64,
    pub outflow: i64,
}

pub struct Invoice {
    pub number: &'static str,
    pub customer: &'static str,
    pub amount_sek: i64,
    pub due_date: &'static str,
    /// Clave del catálogo.
    pub status: &'static str,
    pub severity: Severity,
}

pub struct SieAccountPreview {
    pub account: &'static str,
    /// Clave del catálogo.
    pub name: &'static str,
    pub balance_sek: i64,
}

pub static EXAMPLE_COMPANIES: [Company; 3] = [
    Company {
        id: "a",
        name: "Nordlys Logistik AB",
        org_number: "559012-3456",
        legal_form: "legal.ab",
        sni_code: "52.290",
        sni_text: "sni.52290",
        city: "Göteborg",
        employee_range: "10–19",
        status: "status.active",
        risk_level: Severity::Good,
        signing: "sign.two",
        people: &[
            CompanyPerson { role: "role.ceo", name: "Anna Testsson" },
            CompanyPerson { role: "role.chair", name: "Bo Exempelsson" },
            CompanyPerson { role: "role.member", name: "Cilla Provdotter" },
        ],
        alerts: &[
            CompanyAlert { severity: Severity::Good, text: "alert.report_filed" },
            CompanyAlert { severity: Severity::Warn, text: "alert.board_change" },
        ],
        financials: FinancialHistory {
            revenue: [41200, 45800, 52300, 58900, 64100],
            result: [1900, 2600, 3100, 2800, 4200],
            equity: [6100, 7900, 9800, 11200, 13900],
            total_assets: [14200, 16800, 19500, 21300, 24100],
        },
        benchmarks: CompanyBenchmarks {
            margin: BenchmarkPair { value: 6.5, median: 5.1 },
            solidity: BenchmarkPair { value: 57.7, median: 32.0 },
            liquidity: BenchmarkPair { value: 168.0, median: 120.0 },
        },
    },
    Company {
        id: "b",
        name: "Fjällbruk Bygg & Design AB",
        org_number: "559108-7721",
        legal_form: "legal.ab",
        sni_code: "41.200",
        sni_text: "sni.41200",
        city: "Östersund",
        employee_range: "20–49",
        status: "status.active",
        risk_level: Severity::Bad,
        signing: "sign.alone",
        people: &[
            CompanyPerson { role: "role.ceo", name: "Dan Exempelson" },
            CompanyPerson { role: "role.chair", name: "Eva Testlund" },
        ],
        alerts: &[
            CompanyAlert { severity: Severity::Bad, text: "alert.loss_two_years" },
            CompanyAlert { severity: Severity::Bad, text: "alert.equity_below_half" },
            CompanyAlert { severity: Severity::Warn, text: "alert.new_address" },
        ],
        financials: FinancialHistory {
            revenue: [88300, 91200, 84500, 79800, 72400],
            result: [4700, 3900, 1200, -2100, -3800],
            equity: [12800, 14200, 13100, 9700, 5300],
            total_assets: [38100, 40200, 39800, 37500, 35100],
        },
        benchmarks: CompanyBenchmarks {
            margin: BenchmarkPair { value: -5.2, median: 4.4 },
            solidity: BenchmarkPair { value: 15.1, median: 28.0 },
            liquidity: BenchmarkPair { value: 71.0, median: 115.0 },
        },
    },
    Company {
        id: "c",
        name: "Kvarn & Krydda Livs AB",
        org_number: "559234-9905",
        legal_form: "legal.ab",
        sni_code: "47.290",
        sni_text: "sni.47290",
        city: "Uppsala",
        employee_range: "5–9",
        status: "status.active",
        risk_level: Severity::Warn,
        signing: "sign.alone",
        people: &[
            CompanyPerson { role: "role.ceo", name: "Fredrik Provsson" },
            CompanyPerson { role: "role.chair", name: "Gun Testström" },
        ],
        alerts: &[
            CompanyAlert { severity: Severity::Warn, text: "alert.margin_below_median" },
            CompanyAlert { severity: Severity::Good, text: "alert.no_remarks" },
        ],
        financials: FinancialHistory {
            revenue: [9800, 10400, 11900, 12100, 13300],
            result: [310, 280, 460, 150, 390],
            equity: [900, 1120, 1480, 1550, 1800],
            total_assets: [3400, 3600, 4100, 4300, 4700],
        },
        benchmarks: CompanyBenchmarks {
            margin: BenchmarkPair { value: 2.9, median: 3.6 },
            solidity: BenchmarkPair { value: 38.3, median: 30.0 },
            liquidity: BenchmarkPair { value: 96.0, median: 105.0 },
        },
    },
];

pub fn find_example_company(id_or_org_number: &str) -> Option<&'static Company> {
    EXAMPLE_COMPANIES
        .iter()
        .find(|c| c.id == id_or_org_number || c.org_number == id_or_org_number)
}

pub fn search_example_companies(query: &str) -> Vec<&'static Company> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return EXAMPLE_COMPANIES.iter().collect();
    }
    EXAMPLE_COMPANIES
        .iter()
        .filter(|c| {
            c.name.to_lowercase().contains(&q) || c.org_number.contains(&q) || c.city.to_lowercase().contains(&q)
        })
        .collect()
}

pub static EXAMPLE_WATCH_ALERTS: [WatchAlert; 3] = [
    WatchAlert {
        severity: Severity::Bad,
        company_name: "Fjällbruk Bygg & Design AB",
        text: "watch.loss_registered",
        when: "when.today",
    },
    WatchAlert {
        severity: Severity::Warn,
        company_name: "Kvarn & Krydda Livs AB",
        text: "watch.address_registered",
        when: "when.three_days",
    },
    WatchAlert {
        severity: Severity::Good,
        company_name: "Nordlys Logistik AB",
        text: "watch.report_available",
        when: "when.one_week",
    },
];

/// Entradas y salidas de caja de EJEMPLO para las 13 semanas de la previsión de liquidez.
pub static EXAMPLE_CASH_WEEKS: [CashWeek; 13] = [
    CashWeek { week: 1, inflow: 180, outflow: 140 },
    CashWeek { week: 2, inflow: 90, outflow: 210 },
    CashWeek { week: 3, inflow: 0, outflow: 60 },
    CashWeek { week: 4, inflow: 260, outflow: 120 },
    CashWeek { week: 5, inflow: 120, outflow: 240 },
    CashWeek { week: 6, inflow: 0, outflow: 80 },
    CashWeek { week: 7, inflow: 340, outflow: 110 },
    CashWeek { week: 8, inflow: 60, outflow: 260 },
    CashWeek { week: 9, inflow: 210, outflow: 90 },
    CashWeek { week: 10, inflow: 0, outflow: 180 },
    CashWeek { week: 11, inflow: 150, outflow: 70 },
    CashWeek { week: 12, inflow: 90, outflow: 230 },
    CashWeek { week: 13, inflow: 300, outflow: 100 },
];

pub const EXAMPLE_CASH_START_BALANCE: i64 = 420;

pub static EXAMPLE_INVOICES: [Invoice; 3] = [
    Invoice {
        number: "2024-118",
        customer: "Hamnkraft Test AB",
        amount_sek: 48500,
        due_date: "2025-02-14",
        status: "inv.overdue",
        severity: Severity::Bad,
    },
    Invoice {
        number: "2024-121",
        customer: "Sundsvall Demo AB",
        amount_sek: 22000,
        due_date: "2025-03-02",
        status: "inv.unpaid",
        severity: Severity::Warn,
    },
    Invoice {
        number: "2025-004",
        customer: "Lindqvist Prov AB",
        amount_sek: 75300,
        due_date: "2025-03-20",
        status: "inv.paid",
        severity: Severity::Good,
    },
];

pub static EXAMPLE_SIE_PREVIEW: [SieAccountPreview; 4] = [
    SieAccountPreview { account: "1930", name: "acct.bank", balance_sek: 412300 },
    SieAccountPreview { account: "1510", name: "acct.receivables", balance_sek: 286500 },
    SieAccountPreview { account: "2440", name: "acct.payables", balance_sek: -174900 },
    SieAccountPreview { account: "2610", name: "acct.vat", balance_sek: -61200 },
];
