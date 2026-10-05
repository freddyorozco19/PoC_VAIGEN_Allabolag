//! Equivale a `src/lib/format.ts` y al `toLocaleString("sv-SE")` del original.

/// Entero con separador de miles al estilo sueco (espacio duro, U+00A0) y signo menos correcto (U+2212).
pub fn format_int(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('−');
    }
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('\u{00A0}');
        }
        out.push(ch);
    }
    out
}

/// Formatea miles de kronor (tkr) al estilo sueco.
pub fn format_tkr(n: i64) -> String {
    format_int(n)
}

/// Equivalente a `Math.round` de JS (redondea los empates hacia +infinito).
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands_with_nbsp() {
        assert_eq!(format_int(64100), "64\u{00A0}100");
        assert_eq!(format_int(412300), "412\u{00A0}300");
        assert_eq!(format_int(390), "390");
        assert_eq!(format_int(0), "0");
        assert_eq!(format_int(1234567), "1\u{00A0}234\u{00A0}567");
    }

    #[test]
    fn negative_uses_unicode_minus() {
        assert_eq!(format_tkr(-3800), "−3\u{00A0}800");
        assert_eq!(format_tkr(-174900), "−174\u{00A0}900");
    }

    #[test]
    fn js_round_matches_js_semantics() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(2.4), 2.0);
    }
}
