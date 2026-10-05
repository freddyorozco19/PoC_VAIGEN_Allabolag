//! Equivale a `src/lib/ai-summary.ts`.
//!
//! STUB — no llama a ningún modelo de IA. Reproduce en código el texto de EJEMPLO del mockup original
//! (en los tres idiomas) para que la pestaña de resumen funcione sin backend.

use crate::i18n::{self, Lang};
use crate::model::{Company, Severity};

pub fn generate_example_summary(lang: Lang, company: &Company) -> String {
    let f = &company.financials;
    let growth = (f.revenue[4] as f64 / f.revenue[3] as f64 - 1.0) * 100.0;
    let margin = f.result[4] as f64 / f.revenue[4] as f64 * 100.0;
    let solidity = f.equity[4] as f64 / f.total_assets[4] as f64 * 100.0;
    let tkr = |n: i64| i18n::int(lang, n);

    match company.risk_level {
        Severity::Bad => i18n::tf(
            lang,
            "summary.bad",
            &[&tkr(f.result[3]), &tkr(f.result[4]), &i18n::dec(lang, solidity, 1)],
        ),
        Severity::Warn => i18n::tf(lang, "summary.warn", &[&i18n::dec(lang, growth, 1), &i18n::dec(lang, margin, 1)]),
        Severity::Good => i18n::tf(lang, "summary.good", &[&tkr(f.revenue[4]), &i18n::dec(lang, solidity, 1)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EXAMPLE_COMPANIES;

    #[test]
    fn good_company_summary_in_each_language() {
        let en = generate_example_summary(Lang::En, &EXAMPLE_COMPANIES[0]);
        assert!(en.starts_with("Strong performance."), "{en}");
        assert!(en.contains("64,100") && en.contains("57.7"), "{en}");
        let sv = generate_example_summary(Lang::Sv, &EXAMPLE_COMPANIES[0]);
        assert!(sv.starts_with("Stark utveckling."), "{sv}");
        assert!(sv.contains("64\u{00A0}100") && sv.contains("57,7"), "{sv}");
        let es = generate_example_summary(Lang::Es, &EXAMPLE_COMPANIES[0]);
        assert!(es.starts_with("Evolución sólida."), "{es}");
        assert!(es.contains("64.100") && es.contains("57,7"), "{es}");
    }

    #[test]
    fn bad_company_summary_uses_negative_numbers() {
        let sv = generate_example_summary(Lang::Sv, &EXAMPLE_COMPANIES[1]);
        assert!(sv.contains("(−2\u{00A0}100 och −3\u{00A0}800 tkr)"), "{sv}");
        assert!(sv.contains("15,1"), "{sv}");
        let en = generate_example_summary(Lang::En, &EXAMPLE_COMPANIES[1]);
        assert!(en.contains("−2,100") && en.contains("−3,800") && en.contains("15.1"), "{en}");
    }

    #[test]
    fn warn_company_summary_reports_growth_and_margin() {
        let en = generate_example_summary(Lang::En, &EXAMPLE_COMPANIES[2]);
        assert!(en.contains("(9.9% last year)") && en.contains("2.9%"), "{en}");
    }
}
