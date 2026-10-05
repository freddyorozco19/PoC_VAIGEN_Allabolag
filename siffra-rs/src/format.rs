//! Formato numérico. Los formatos por idioma están en `i18n.rs` (`int`, `dec`, `num`, `pct1`, `pct`).

/// Equivalente a `Math.round` de JS (redondea los empates hacia +infinito).
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_round_matches_js_semantics() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(2.4), 2.0);
    }
}
