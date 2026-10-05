//! Medianas del sector (SNI) desde la base de estadísticas ABIERTA de SCB (PxWebApi v2).
//! Equivale a `src/lib/data-sources/scb/statistics.ts` del proyecto Next.js. Sin clave ni registro.
//!
//! Tabla `TAB1270` "Branschnyckeltal efter näringsgren SNI 2007 och storleksklass och kvartil":
//! - `margin`    ← Nettomarginal (0000032H): nettoresultat en % de la facturación.
//! - `solidity`  ← Soliditet (00000340): patrimonio AJUSTADO / balance total (SCB suma parte de las
//!                 reservas no tributadas; difiere algo del cálculo simple de la app).
//! - `liquidity` ← Kassalikviditet (00000343).
//!
//! SCB solo publica un valor si hay suficientes empresas, así que se cae a un SNI más general
//! (52.290 → 52.29 → 52.2 → 52) y, si no hay datos para ese tamaño, al total de tamaños.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::i18n::Lang;

const TABLE: &str = "TAB1270";
const CODE_MARGIN: &str = "0000032H";
const CODE_SOLIDITY: &str = "00000340";
const CODE_LIQUIDITY: &str = "00000343";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const SIZE_CLASSES: [&str; 8] = ["001", "1-4", "5-9", "10-19", "20-49", "50-99", "100-199", "200-499"];

#[derive(Clone, Debug, PartialEq)]
pub struct SectorMedians {
    pub margin: f64,
    pub solidity: f64,
    pub liquidity: f64,
    /// Año de los datos, p.ej. "2024".
    pub year: String,
    /// Código SNI realmente usado (puede ser más general que el pedido) y su descripción.
    pub sni_code: String,
    pub sni_label: String,
    /// Clase de tamaño usada: "10-19", o "TOT" (todos los tamaños).
    pub size_class: String,
    pub exact_sni: bool,
    pub exact_size: bool,
}

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("siffra-rs/0.1 (PoC)")
        .build()
        .expect("cliente HTTP")
});

type CacheMap = HashMap<String, (Instant, Option<SectorMedians>)>;
static CACHE: LazyLock<Mutex<CacheMap>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn base_url() -> String {
    std::env::var("SCB_STATS_BASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://statistikdatabasen.scb.se/api/v2".to_string())
        .trim_end_matches('/')
        .to_string()
}

/// `SCB_STATS_DISABLED=1` → no llamar a SCB; la ficha usa las medianas de EJEMPLO.
pub fn enabled() -> bool {
    std::env::var("SCB_STATS_DISABLED").map(|v| v != "1").unwrap_or(true)
}

/// "52.290 Övriga…" → ["52.290", "52.29", "52.2", "52"] (del más específico al más general).
pub fn sni_candidates(sni: &str) -> Vec<String> {
    let s = sni.trim();
    let head: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if head.len() != 2 {
        return vec![];
    }
    let rest: String = s[2..]
        .strip_prefix('.')
        .unwrap_or(&s[2..])
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .take(3)
        .collect();
    let digits = format!("{head}{rest}");
    (2..=digits.len())
        .rev()
        .map(|len| {
            let d = &digits[..len];
            if len <= 2 { d.to_string() } else { format!("{}.{}", &d[..2], &d[2..]) }
        })
        .collect()
}

/// "10–19" (con raya) → "10-19". `None` si SCB no tiene esa clase de tamaño.
pub fn size_class_from_employee_range(range: &str) -> Option<&'static str> {
    let normalised = range.trim().replace(['–', '—'], "-");
    let code = if normalised == "0" { "001" } else { normalised.as_str() };
    SIZE_CLASSES.iter().copied().find(|c| *c == code)
}

/// Lee una respuesta json-stat2 de SCB. `None` = celda suprimida o sin las tres medianas.
pub fn parse_medians(json: &Value, sni: &str, size_class: &str) -> Option<SectorMedians> {
    let idx = json.pointer("/dimension/ContentsCode/category/index")?;
    let values = json.get("value")?.as_array()?;
    let pick = |code: &str| -> Option<f64> {
        let i = idx.get(code)?.as_u64()? as usize;
        values.get(i)?.as_f64().filter(|v| v.is_finite())
    };
    let (margin, solidity, liquidity) = (pick(CODE_MARGIN)?, pick(CODE_SOLIDITY)?, pick(CODE_LIQUIDITY)?);

    let year = json
        .pointer("/dimension/Tid/category/index")
        .and_then(Value::as_object)
        .and_then(|o| o.keys().next().cloned())
        .unwrap_or_default();
    let sni_label = json
        .pointer("/dimension/SNI2007/category/label")
        .and_then(|l| l.get(sni))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    Some(SectorMedians {
        margin,
        solidity,
        liquidity,
        year,
        sni_code: sni.to_string(),
        sni_label,
        size_class: size_class.to_string(),
        exact_sni: false, // los fija get_sector_medians
        exact_size: false,
    })
}

/// `Ok(None)` = SCB no tiene ese dato (código inexistente o celda suprimida). `Err` si SCB falla.
async fn fetch_medians(sni: &str, size_class: &str, lang: Lang) -> Result<Option<SectorMedians>, String> {
    let contents = [CODE_MARGIN, CODE_SOLIDITY, CODE_LIQUIDITY].join(",");
    let res = CLIENT
        .get(format!("{}/tables/{TABLE}/data", base_url()))
        .query(&[
            ("lang", lang.scb_lang()),
            ("outputFormat", "json-stat2"),
            ("valueCodes[SNI2007]", sni),
            ("valueCodes[Storleksklass]", size_class),
            ("valueCodes[AKvartil]", "Med"),
            ("valueCodes[ContentsCode]", contents.as_str()),
            ("valueCodes[Tid]", "top(1)"),
        ])
        .send()
        .await
        .map_err(|e| format!("SCB: no se pudo consultar {TABLE} ({sni}, {size_class}): {e}"))?;

    let status = res.status();
    // 400 "Non-existent value": ese código SNI / tamaño no existe en la tabla.
    if status.as_u16() == 400 || status.as_u16() == 404 {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(format!("SCB: error HTTP {status} al consultar {TABLE} ({sni}, {size_class})."));
    }
    let json: Value = res.json().await.map_err(|e| format!("SCB: respuesta no válida: {e}"))?;
    Ok(parse_medians(&json, sni, size_class))
}

fn cache_key(first: &str, wanted: Option<&str>, lang: Lang) -> String {
    // Las etiquetas de SCB solo existen en sueco e inglés: la caché distingue entre esos dos.
    format!("{first}|{}|{}", wanted.unwrap_or("TOT"), lang.scb_lang())
}

/// Mira la caché sin llamar a SCB. `Some(valor)` si ya hay un resultado fresco (que puede ser
/// "sin datos" = `Some(None)`); `None` si habría que preguntar a SCB (la ficha usa un esqueleto de carga).
pub fn peek(sni: &str, employee_range: &str, lang: Lang) -> Option<Option<SectorMedians>> {
    let candidates = sni_candidates(sni);
    let first = candidates.first()?;
    let key = cache_key(first, size_class_from_employee_range(employee_range), lang);
    let guard = CACHE.lock().unwrap();
    let (at, value) = guard.get(&key)?;
    (at.elapsed() < CACHE_TTL).then(|| value.clone())
}

/// Medianas del sector para una empresa, según su SNI y su tramo de empleados.
/// `Ok(None)` si SCB no tiene datos en ningún nivel. `Err` si SCB no responde.
pub async fn get_sector_medians(sni: &str, employee_range: &str, lang: Lang) -> Result<Option<SectorMedians>, String> {
    let candidates = sni_candidates(sni);
    let Some(first) = candidates.first() else { return Ok(None) };
    let wanted = size_class_from_employee_range(employee_range);

    let key = cache_key(first, wanted, lang);
    if let Some((at, value)) = CACHE.lock().unwrap().get(&key) {
        if at.elapsed() < CACHE_TTL {
            return Ok(value.clone());
        }
    }

    // Orden de preferencia: mismo tamaño en niveles SNI cada vez más generales; después, todos los tamaños.
    let sizes: Vec<&str> = match wanted {
        Some(w) => vec![w, "TOT"],
        None => vec!["TOT"],
    };
    let mut result = None;
    'search: for size in sizes {
        for code in &candidates {
            if let Some(mut found) = fetch_medians(code, size, lang).await? {
                found.exact_sni = code == first;
                found.exact_size = Some(size) == wanted;
                result = Some(found);
                break 'search;
            }
        }
    }

    CACHE.lock().unwrap().insert(key, (Instant::now(), result.clone()));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sni_candidates_go_from_specific_to_general() {
        assert_eq!(sni_candidates("52.290 Övriga stödtjänster"), ["52.290", "52.29", "52.2", "52"]);
        assert_eq!(sni_candidates("47.290 x"), ["47.290", "47.29", "47.2", "47"]);
        assert_eq!(sni_candidates("47.2"), ["47.2", "47"]);
        assert!(sni_candidates("zz").is_empty());
        assert!(sni_candidates("").is_empty());
    }

    #[test]
    fn size_class_mapping() {
        assert_eq!(size_class_from_employee_range("10–19"), Some("10-19"));
        assert_eq!(size_class_from_employee_range("5–9"), Some("5-9"));
        assert_eq!(size_class_from_employee_range("0"), Some("001"));
        assert_eq!(size_class_from_employee_range("500+"), None);
    }

    /// Forma real de la respuesta (recortada) de TAB1270 para 52.290 / 10-19 / 2024.
    fn sample() -> Value {
        json!({
            "dimension": {
                "SNI2007": {"category": {"index": {"52.290": 0}, "label": {"52.290": "Övriga serviceföretag till transport"}}},
                "ContentsCode": {"category": {"index": {"0000032H": 0, "00000340": 1, "00000343": 2}}},
                "Tid": {"category": {"index": {"2024": 0}}}
            },
            "value": [2.8, 32, 142]
        })
    }

    #[test]
    fn parses_real_shaped_response() {
        let m = parse_medians(&sample(), "52.290", "10-19").unwrap();
        assert_eq!((m.margin, m.solidity, m.liquidity), (2.8, 32.0, 142.0));
        assert_eq!(m.year, "2024");
        assert_eq!(m.sni_label, "Övriga serviceföretag till transport");
    }

    #[test]
    fn suppressed_cell_is_none() {
        let mut j = sample();
        j["value"] = json!([2.8, null, 142]);
        assert!(parse_medians(&j, "52.290", "10-19").is_none());
        j["value"] = json!([]);
        assert!(parse_medians(&j, "52.290", "10-19").is_none());
    }

    /// Contra el API real de SCB: `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "requiere red"]
    async fn live_scb_lookup() {
        let m = get_sector_medians("52.290 x", "10–19", Lang::En).await.unwrap().unwrap();
        assert!(m.exact_sni && m.exact_size);
        assert!(m.liquidity > 0.0);
        let fallback = get_sector_medians("47.290 x", "5–9", Lang::Sv).await.unwrap().unwrap();
        assert_eq!(fallback.sni_code, "47.29");
        assert!(!fallback.exact_sni);
        assert!(get_sector_medians("99.999 x", "10–19", Lang::Es).await.unwrap().is_none());
    }
}
