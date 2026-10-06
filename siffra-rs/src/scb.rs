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
use std::sync::atomic::{AtomicBool, Ordering};
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
/// SCB retiró la versión en inglés de la tabla (responde "Non-existent table"): una vez visto, se consulta directamente en sueco.
static ENGLISH_VERSION_GONE: AtomicBool = AtomicBool::new(false);

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
    // Acepta "52.290 Övriga…" (con punto, como en SCB) y "52290" (sin punto, como lo entrega Bolagsverket).
    let digits: String = sni.trim().chars().take_while(|c| c.is_ascii_digit() || *c == '.').filter(char::is_ascii_digit).take(5).collect();
    if digits.len() < 2 {
        return vec![];
    }
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
    let mut api_lang = lang.scb_lang();
    if api_lang == "en" && ENGLISH_VERSION_GONE.load(Ordering::Relaxed) {
        api_lang = "sv";
    }
    let res = loop {
        let res = CLIENT
            .get(format!("{}/tables/{TABLE}/data", base_url()))
            .query(&[
                ("lang", api_lang),
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
        let status = res.status().as_u16();
        // 400/404 "Non-existent value": ese código SNI / tamaño no existe en la tabla.
        // 400/404 "Non-existent table": esa versión de idioma de la tabla no existe (SCB retiró la inglesa).
        if status == 400 || status == 404 {
            let body = res.text().await.unwrap_or_default();
            if api_lang != "sv" && body.contains("Non-existent table") {
                if !ENGLISH_VERSION_GONE.swap(true, Ordering::Relaxed) {
                    eprintln!("SCB: la tabla {TABLE} ya no existe en inglés; se consulta en sueco.");
                }
                api_lang = "sv";
                continue;
            }
            return Ok(None);
        }
        break res;
    };
    let status = res.status();
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
        assert_eq!(sni_candidates("71121"), ["71.121", "71.12", "71.1", "71"], "Bolagsverket da el código sin punto");
        assert_eq!(sni_candidates("5229"), ["52.29", "52.2", "52"]);
        assert!(sni_candidates("7").is_empty());
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


    /// SCB retiró la versión en inglés de TAB1270 (pedirla da "Non-existent table") y las medianas dejaron de
    /// salir, en silencio, para quien usa español o inglés. Contra un SCB simulado: se reintenta en sueco y, una
    /// vez visto, ya no se vuelve a pedir en inglés.
    #[tokio::test]
    async fn falls_back_to_swedish_when_the_english_table_is_gone() {
        use axum::extract::Query;
        use axum::http::StatusCode;
        use axum::response::IntoResponse;
        use std::sync::atomic::AtomicUsize;
        static ENGLISH_HITS: AtomicUsize = AtomicUsize::new(0);

        async fn data(Query(q): Query<HashMap<String, String>>) -> axum::response::Response {
            if q.get("lang").map(String::as_str) == Some("en") {
                ENGLISH_HITS.fetch_add(1, Ordering::SeqCst);
                let body = r#"{"type":"Parameter error","title":"Non-existent table","status":404}"#;
                return (StatusCode::BAD_REQUEST, [("content-type", "application/json")], body).into_response();
            }
            let sni = q.get("valueCodes[SNI2007]").cloned().unwrap_or_default();
            let body = json!({
                "dimension": {
                    "SNI2007": {"category": {"index": {sni.clone(): 0}, "label": {sni.clone(): "Teknisk konsultverksamhet"}}},
                    "ContentsCode": {"category": {"index": {"0000032H": 0, "00000340": 1, "00000343": 2}}},
                    "Tid": {"category": {"index": {"2024": 0}}}
                },
                "value": [4.5, 38, 130]
            });
            axum::Json(body).into_response()
        }
        let app = axum::Router::new().route("/tables/TAB1270/data", axum::routing::get(data));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        std::env::set_var("SCB_STATS_BASE_URL", format!("http://{addr}"));

        let m = get_sector_medians("71.121 x", "", Lang::En).await.unwrap().expect("las medianas salen aunque la tabla inglesa no exista");
        assert_eq!((m.margin, m.solidity, m.sni_code.as_str()), (4.5, 38.0, "71.121"));
        assert!(m.exact_sni);
        assert_eq!(ENGLISH_HITS.load(Ordering::SeqCst), 1, "el primer intento en inglés falla una sola vez");
        // Español también (usa la versión inglesa de SCB) y sin volver a pedir inglés.
        let es = get_sector_medians("62.010 x", "", Lang::Es).await.unwrap().unwrap();
        assert_eq!(es.solidity, 38.0);
        assert_eq!(ENGLISH_HITS.load(Ordering::SeqCst), 1, "ya se sabe que la tabla inglesa no existe");
        std::env::remove_var("SCB_STATS_BASE_URL");
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
