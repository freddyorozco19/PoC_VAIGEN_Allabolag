//! Idiomas (español, inglés, sueco): catálogo de textos y formato numérico por idioma.
//!
//! Los textos viven en `catalog.rs` como `(clave, es, en, sv)`. `t()` devuelve el texto; `tf()` además
//! sustituye `{0}`, `{1}`… Un test comprueba que cada clave usada en el código existe y que no falta
//! ninguna traducción. Los datos que vienen de Bolagsverket (descripciones de actividad, nombres) son
//! texto original en sueco y no se traducen.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use crate::catalog::ENTRIES;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Lang {
    Es,
    En,
    Sv,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::Es, Lang::En, Lang::Sv];
    pub const DEFAULT: Lang = Lang::Es;

    pub fn code(self) -> &'static str {
        match self {
            Lang::Es => "es",
            Lang::En => "en",
            Lang::Sv => "sv",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        match code.trim().to_ascii_lowercase().as_str() {
            "es" => Some(Lang::Es),
            "en" => Some(Lang::En),
            "sv" => Some(Lang::Sv),
            _ => None,
        }
    }

    /// Valor del atributo `lang` de `<html>`.
    pub fn html_lang(self) -> &'static str {
        self.code()
    }

    /// Nombre del idioma en su propio idioma.
    pub fn name(self) -> &'static str {
        match self {
            Lang::Es => "Español",
            Lang::En => "English",
            Lang::Sv => "Svenska",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Lang::Es => "ES",
            Lang::En => "EN",
            Lang::Sv => "SV",
        }
    }

    /// Idioma de las etiquetas de la API de estadísticas de SCB (solo existen en sueco e inglés).
    pub fn scb_lang(self) -> &'static str {
        match self {
            Lang::Sv => "sv",
            _ => "en",
        }
    }

    fn index(self) -> usize {
        match self {
            Lang::Es => 0,
            Lang::En => 1,
            Lang::Sv => 2,
        }
    }

    fn thousands(self) -> char {
        match self {
            Lang::Es => '.',
            Lang::En => ',',
            Lang::Sv => '\u{00A0}',
        }
    }

    fn decimal(self) -> char {
        match self {
            Lang::En => '.',
            _ => ',',
        }
    }
}

/// Elige idioma a partir de la cabecera `Accept-Language` (primer idioma soportado por orden de preferencia).
pub fn from_accept_language(header: &str) -> Option<Lang> {
    let mut candidates: Vec<(f32, Lang)> = header
        .split(',')
        .filter_map(|part| {
            let mut it = part.trim().split(';');
            let tag = it.next()?.trim();
            let q = it.find_map(|p| p.trim().strip_prefix("q=").and_then(|v| v.parse::<f32>().ok())).unwrap_or(1.0);
            let lang = Lang::from_code(tag.split('-').next()?)?;
            (q > 0.0).then_some((q, lang))
        })
        .collect();
    candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    candidates.first().map(|(_, l)| *l)
}

static CATALOG: LazyLock<HashMap<&'static str, [&'static str; 3]>> =
    LazyLock::new(|| ENTRIES.iter().map(|(k, es, en, sv)| (*k, [*es, *en, *sv])).collect());

static MISSING: LazyLock<Mutex<HashMap<String, &'static str>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Texto traducido. Una clave inexistente se muestra como `⟦clave⟧` (y un test la detecta).
pub fn t(lang: Lang, key: &str) -> &'static str {
    match CATALOG.get(key) {
        Some(texts) => texts[lang.index()],
        None => {
            let mut missing = MISSING.lock().unwrap();
            missing.entry(key.to_string()).or_insert_with(|| Box::leak(format!("⟦{key}⟧").into_boxed_str()))
        }
    }
}

/// Como `t`, sustituyendo `{0}`, `{1}`… por los argumentos.
pub fn tf(lang: Lang, key: &str, args: &[&str]) -> String {
    let mut text = t(lang, key).to_string();
    for (i, a) in args.iter().enumerate() {
        text = text.replace(&format!("{{{i}}}"), a);
    }
    text
}

pub fn has_key(key: &str) -> bool {
    CATALOG.contains_key(key)
}

// ───────────── Números según el idioma ─────────────

fn group_digits(digits: &str, sep: char) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

/// Entero con separador de miles del idioma y signo menos tipográfico (U+2212).
pub fn int(lang: Lang, n: i64) -> String {
    let digits = group_digits(&n.unsigned_abs().to_string(), lang.thousands());
    if n < 0 { format!("−{digits}") } else { digits }
}

/// Número con `decimals` decimales fijos, con separadores del idioma y sin "−0,0".
pub fn dec(lang: Lang, v: f64, decimals: usize) -> String {
    let factor = 10f64.powi(decimals as i32);
    let rounded = (v.abs() * factor).round() / factor;
    let text = format!("{rounded:.decimals$}");
    let (whole, frac) = text.split_once('.').unwrap_or((&text, ""));
    let mut out = group_digits(whole, lang.thousands());
    if decimals > 0 {
        out.push(lang.decimal());
        out.push_str(frac);
    }
    if v < 0.0 && rounded > 0.0 { format!("−{out}") } else { out }
}

/// Número con los decimales mínimos necesarios (6,5 · 32 · 57,7), con el separador decimal del idioma.
pub fn num(lang: Lang, v: f64) -> String {
    let s = format!("{v}");
    let (whole, frac) = s.split_once('.').unwrap_or((&s, ""));
    let neg = whole.starts_with('-');
    let digits = group_digits(whole.trim_start_matches('-'), lang.thousands());
    let mut out = if neg { format!("−{digits}") } else { digits };
    if !frac.is_empty() {
        out.push(lang.decimal());
        out.push_str(frac);
    }
    out
}

/// Porcentaje con un decimal: "45,2 %" (es, sv) o "45.2%" (en).
pub fn pct1(lang: Lang, v: f64) -> String {
    match lang {
        Lang::En => format!("{}%", dec(lang, v, 1)),
        _ => format!("{}\u{00A0}%", dec(lang, v, 1)),
    }
}

/// Porcentaje entero o con los decimales justos, para las barras de comparación.
pub fn pct(lang: Lang, v: f64) -> String {
    match lang {
        Lang::En => format!("{}%", num(lang, v)),
        _ => format!("{}\u{00A0}%", num(lang, v)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_codes_round_trip() {
        for l in Lang::ALL {
            assert_eq!(Lang::from_code(l.code()), Some(l));
        }
        assert_eq!(Lang::from_code("ES"), Some(Lang::Es));
        assert_eq!(Lang::from_code("fr"), None);
    }

    #[test]
    fn accept_language_picks_the_preferred_supported_language() {
        assert_eq!(from_accept_language("sv-SE,sv;q=0.9,en;q=0.8"), Some(Lang::Sv));
        assert_eq!(from_accept_language("fr-FR,fr;q=0.9,en;q=0.8"), Some(Lang::En));
        assert_eq!(from_accept_language("es-CO,es;q=0.9"), Some(Lang::Es));
        assert_eq!(from_accept_language("de,fr"), None);
        assert_eq!(from_accept_language(""), None);
        assert_eq!(from_accept_language("en;q=0.2, sv;q=0.8"), Some(Lang::Sv));
    }

    #[test]
    fn integers_by_locale() {
        assert_eq!(int(Lang::Sv, 64_100), "64\u{00A0}100");
        assert_eq!(int(Lang::En, 64_100), "64,100");
        assert_eq!(int(Lang::Es, 64_100), "64.100");
        assert_eq!(int(Lang::En, -3_800), "−3,800");
        assert_eq!(int(Lang::Es, 0), "0");
        assert_eq!(int(Lang::En, 1_234_567), "1,234,567");
    }

    #[test]
    fn decimals_and_percentages_by_locale() {
        assert_eq!(dec(Lang::Sv, 1234.56, 1), "1\u{00A0}234,6");
        assert_eq!(dec(Lang::En, 1234.56, 1), "1,234.6");
        assert_eq!(dec(Lang::En, -0.04, 1), "0.0", "nunca −0.0");
        assert_eq!(pct1(Lang::Es, 45.2), "45,2\u{00A0}%");
        assert_eq!(pct1(Lang::En, 45.2), "45.2%");
        assert_eq!(pct1(Lang::Sv, -45.31), "−45,3\u{00A0}%");
        assert_eq!(num(Lang::Es, 6.5), "6,5");
        assert_eq!(num(Lang::En, 32.0), "32");
        assert_eq!(num(Lang::Sv, -5.2), "−5,2");
        assert_eq!(pct(Lang::En, 168.0), "168%");
    }

    #[test]
    fn placeholders_are_replaced() {
        assert_eq!(tf(Lang::En, "test.hello", &["Ana", "3"]), "Hello Ana, you have 3");
    }

    #[test]
    fn missing_keys_are_visible_not_silent() {
        assert_eq!(t(Lang::Es, "no.existe.nunca"), "⟦no.existe.nunca⟧");
        assert!(!has_key("no.existe.nunca"));
    }
}
