//! Equivale a `src/lib/ai-summary.ts`.
//!
//! STUB — no llama a ningún modelo de IA. Reproduce en código el texto de
//! EJEMPLO del mockup original para que la pestaña "Sammanfattning" funcione
//! sin backend.

use crate::format::format_tkr;
use crate::model::{Company, Severity};

pub fn generate_example_summary(company: &Company) -> String {
    let f = &company.financials;
    let growth = (f.revenue[4] as f64 / f.revenue[3] as f64 - 1.0) * 100.0;
    let margin = f.result[4] as f64 / f.revenue[4] as f64 * 100.0;
    let solidity = f.equity[4] as f64 / f.total_assets[4] as f64 * 100.0;

    match company.risk_level {
        Severity::Bad => format!(
            "Omsättningen har fallit tre år i rad och bolaget redovisar förlust de två senaste åren ({} och {} tkr). Soliditeten är {:.1} %, under branschens median. Eget kapital har mer än halverats sedan 2022. Kräver närmare granskning innan kreditgivning.",
            format_tkr(f.result[3]),
            format_tkr(f.result[4]),
            solidity
        ),
        Severity::Warn => format!(
            "Stabil men liten verksamhet. Omsättningen växer ({:.1} % senaste året), men vinstmarginalen på {:.1} % ligger under branschens median. Soliditeten är god.",
            growth, margin
        ),
        Severity::Good => format!(
            "Stark utveckling. Omsättningen har vuxit varje år, till {} tkr, och soliditeten är {:.1} %, över branschens median. Inga varningssignaler i underlaget.",
            format_tkr(f.revenue[4]),
            solidity
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EXAMPLE_COMPANIES;

    #[test]
    fn good_company_summary() {
        let s = generate_example_summary(&EXAMPLE_COMPANIES[0]);
        assert!(s.starts_with("Stark utveckling."));
        assert!(s.contains("64\u{00A0}100 tkr"));
        assert!(s.contains("57,7") || s.contains("57.7 %"));
    }

    #[test]
    fn bad_company_summary() {
        let s = generate_example_summary(&EXAMPLE_COMPANIES[1]);
        assert!(s.contains("(−2\u{00A0}100 och −3\u{00A0}800 tkr)"));
        assert!(s.contains("15.1 %"));
    }

    #[test]
    fn warn_company_summary() {
        let s = generate_example_summary(&EXAMPLE_COMPANIES[2]);
        assert!(s.contains("(9.9 % senaste året)"));
        assert!(s.contains("2.9 %"));
    }
}
