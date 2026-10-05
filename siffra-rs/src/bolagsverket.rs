//! Cliente del API gratuito de Bolagsverket ("värdefulla datamängder"). Equivale a
//! `src/lib/data-sources/bolagsverket/client.ts` del proyecto Next.js.
//!
//! Contrato (OpenAPI oficial, publicado en el portal de desarrolladores de Bolagsverket):
//! OAuth2 client_credentials con el scope `vardefulla-datamangder:read` y
//! `POST {base}/organisationer` con `{ "identitetsbeteckning": "NNNNNNNNNN" }`. Los errores por fuente
//! de datos llegan DENTRO de un HTTP 200 (campo `fel` de cada sección). Límite: 60 peticiones/minuto.
//!
//! Entornos (según los correos "Anslutningsuppgifter" de Bolagsverket):
//! - test:       base `https://gw-accept2.api.bolagsverket.se/vardefulla-datamangder/v1`
//!               token `https://portal-accept2.api.bolagsverket.se/oauth2/token`
//! - producción: base `https://gw.api.bolagsverket.se/vardefulla-datamangder/v1`
//!               token `https://portal.api.bolagsverket.se/oauth2/token`
//!
//! Solo cubre datos básicos de la empresa; las cuentas anuales (`/dokumentlista`, `/dokument`) no.

use std::collections::HashMap;
use std::fmt;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const SCOPE: &str = "vardefulla-datamangder:read";
const ORG_CACHE_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug)]
pub enum BvError {
    /// No existe una organización con ese número.
    NotFound(String),
    /// El identificador no es un organisationsnummer válido (10 dígitos).
    Invalid(String),
    /// Configuración, red, autenticación, límite de uso o respuesta inesperada.
    Upstream { status: Option<u16>, message: String },
}

impl fmt::Display for BvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BvError::NotFound(n) => write!(f, "Bolagsverket: no hay ninguna organización con el número {n}."),
            BvError::Invalid(m) | BvError::Upstream { message: m, .. } => write!(f, "{m}"),
        }
    }
}

fn upstream(status: Option<u16>, message: impl Into<String>) -> BvError {
    BvError::Upstream { status, message: message.into() }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Organisation {
    /// 10 dígitos, sin guion.
    pub organisationsnummer: String,
    pub namn: String,
    pub organisationsform: String,
    /// (código, descripción)
    pub sni: Vec<(String, String)>,
    pub gatuadress: Option<String>,
    pub postnummer: Option<String>,
    pub postort: Option<String>,
    /// "AAAA-MM-DD" o vacío si no consta.
    pub registreringsdatum: String,
    pub avregistreringsdatum: Option<String>,
    /// false si está avregistrerad o marcada como inactiva (verksamOrganisation = NEJ).
    pub aktiv: bool,
    /// Procedimientos en curso, p.ej. "Konkurs (sedan 2024-01-26)".
    pub forfaranden: Vec<String>,
    pub verksamhetsbeskrivning: Option<String>,
}

impl Organisation {
    /// "NNNNNN-NNNN"
    pub fn formatted_number(&self) -> String {
        let n = &self.organisationsnummer;
        if n.len() == 10 { format!("{}-{}", &n[..6], &n[6..]) } else { n.clone() }
    }
}

// ───────────── Configuración ─────────────

#[derive(Clone)]
pub struct Config {
    client_id: String,
    client_secret: String,
    base_url: String,
    token_url: String,
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

pub fn load_config() -> Option<Config> {
    let client_id = env_nonempty("BOLAGSVERKET_CLIENT_ID")?;
    let client_secret = env_nonempty("BOLAGSVERKET_CLIENT_SECRET")?;
    let base_url = env_nonempty("BOLAGSVERKET_BASE_URL")?.trim_end_matches('/').to_string();
    let token_url = env_nonempty("BOLAGSVERKET_TOKEN_URL").unwrap_or_else(|| default_token_url(&base_url));
    Some(Config { client_id, client_secret, base_url, token_url })
}

pub fn configured() -> bool {
    load_config().is_some()
}

/// Host de la URL base, para mostrar a qué entorno se conecta (sin credenciales).
pub fn environment_host() -> Option<String> {
    let base = env_nonempty("BOLAGSVERKET_BASE_URL")?;
    Some(base.split("://").nth(1)?.split('/').next()?.to_string())
}

/// El token no se pide al gateway sino al portal del mismo entorno: `gw` → `portal` en el host
/// (`gw.api…` → `portal.api…`, `gw-accept2.api…` → `portal-accept2.api…`). Para cualquier otro host
/// (p.ej. un servidor local de pruebas) se usa `/oauth2/token` en ese mismo host.
pub fn default_token_url(base_url: &str) -> String {
    let (scheme, rest) = base_url.split_once("://").unwrap_or(("https", base_url));
    let host = rest.split('/').next().unwrap_or(rest);
    let host = match host.strip_prefix("gw") {
        Some(tail) if tail.starts_with('.') || tail.starts_with('-') => format!("portal{tail}"),
        _ => host.to_string(),
    };
    format!("{scheme}://{host}/oauth2/token")
}

/// Normaliza un organisationsnummer a 10 dígitos (acepta "NNNNNN-NNNN"). Rechaza a propósito los
/// 12 dígitos: el API también acepta personnummer (empresarios individuales) y esta app no consulta
/// datos personales.
pub fn normalize_org_number(input: &str) -> Option<String> {
    let digits: String = input.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    (digits.len() == 10 && digits.chars().all(|c| c.is_ascii_digit())).then_some(digits)
}

/// Dígito de control (Luhn) de un organisationsnummer de 10 dígitos. Evita consultar números imposibles.
pub fn luhn_valid(digits: &str) -> bool {
    let sum: u32 = digits
        .chars()
        .filter_map(|c| c.to_digit(10))
        .enumerate()
        .map(|(i, d)| if i % 2 == 0 { let x = d * 2; x / 10 + x % 10 } else { d })
        .sum();
    sum % 10 == 0
}

// ───────────── HTTP, token y caché ─────────────

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("siffra-rs/0.1 (PoC)")
        .build()
        .expect("cliente HTTP")
});

static TOKEN: LazyLock<Mutex<Option<(String, Instant)>>> = LazyLock::new(|| Mutex::new(None));
static ORG_CACHE: LazyLock<Mutex<HashMap<String, (Instant, Option<Organisation>)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

async fn access_token(config: &Config) -> Result<String, BvError> {
    if let Some((token, expires_at)) = TOKEN.lock().unwrap().as_ref() {
        if *expires_at > Instant::now() + Duration::from_secs(30) {
            return Ok(token.clone());
        }
    }
    let res = CLIENT
        .post(&config.token_url)
        .header("Accept", "application/json")
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("scope", SCOPE),
        ])
        .send()
        .await
        .map_err(|e| upstream(None, format!("Bolagsverket: no se pudo contactar con el servidor de tokens: {e}")))?;
    if !res.status().is_success() {
        let code = res.status().as_u16();
        return Err(upstream(
            Some(code),
            format!("Bolagsverket: no se pudo obtener el token OAuth2 (HTTP {code}). Revisa las credenciales y que correspondan al entorno de BOLAGSVERKET_BASE_URL."),
        ));
    }
    let json: Value = res.json().await.map_err(|e| upstream(None, format!("Bolagsverket: respuesta de token no válida: {e}")))?;
    let token = json
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| upstream(None, "Bolagsverket: la respuesta del token no incluye access_token."))?
        .to_string();
    let ttl = json.get("expires_in").and_then(Value::as_u64).unwrap_or(300);
    *TOKEN.lock().unwrap() = Some((token.clone(), Instant::now() + Duration::from_secs(ttl)));
    Ok(token)
}

/// Consulta una organización por organisationsnummer. Cacheada 10 min para respetar el límite de 60/min.
pub async fn get_organisation_by_number(input: &str) -> Result<Organisation, BvError> {
    let id = normalize_org_number(input)
        .ok_or_else(|| BvError::Invalid(format!("Bolagsverket: \"{input}\" no es un organisationsnummer válido (10 dígitos).")))?;

    if !luhn_valid(&id) {
        return Err(BvError::Invalid(format!("Bolagsverket: {id} no tiene un dígito de control válido.")));
    }

    if let Some((at, value)) = ORG_CACHE.lock().unwrap().get(&id) {
        if at.elapsed() < ORG_CACHE_TTL {
            return value.clone().ok_or(BvError::NotFound(id));
        }
    }

    let config = load_config().ok_or_else(|| {
        upstream(None, "Bolagsverket: faltan BOLAGSVERKET_CLIENT_ID / BOLAGSVERKET_CLIENT_SECRET / BOLAGSVERKET_BASE_URL (ver .env.example).")
    })?;
    let token = access_token(&config).await?;

    let res = CLIENT
        .post(format!("{}/organisationer", config.base_url))
        .bearer_auth(&token)
        .header("Accept", "application/json")
        .json(&json!({ "identitetsbeteckning": id }))
        .send()
        .await
        .map_err(|e| upstream(None, format!("Bolagsverket: no se pudo consultar {id}: {e}")))?;

    let status = res.status().as_u16();
    match status {
        401 => {
            *TOKEN.lock().unwrap() = None; // el próximo intento pedirá un token nuevo
            return Err(upstream(Some(401), "Bolagsverket: token rechazado (401). Reintenta."));
        }
        403 => return Err(upstream(Some(403), "Bolagsverket: acceso denegado (403). Comprueba que las credenciales son del entorno correcto (test/producción) y tienen el scope vardefulla-datamangder:read.")),
        429 => return Err(upstream(Some(429), "Bolagsverket: límite de 60 peticiones/minuto superado.")),
        404 => return remember(id, Err(BvError::NotFound(String::new()))),
        s if !(200..300).contains(&s) => {
            // El cuerpo del error (formato `ApiError` de la especificación) ayuda a diagnosticar; es del servidor.
            let body = res.text().await.unwrap_or_default();
            // 400 "Ogiltig identitetsbeteckning": ese número no es válido / no existe → "no encontrada", no un fallo.
            if s == 400 && body.to_lowercase().contains("identitetsbeteckning") {
                return Err(BvError::Invalid(format!("Bolagsverket: {id} no es una identitetsbeteckning válida.")));
            }
            let detail: String = body.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(300).collect();
            return Err(upstream(Some(s), format!("Bolagsverket: error HTTP {s} al consultar {id}. {detail}").trim_end().to_string()));
        }
        _ => {}
    }

    let raw: Value = res.json().await.map_err(|e| upstream(Some(status), format!("Bolagsverket: respuesta no válida: {e}")))?;
    remember(id.clone(), map_organisation(&raw, &id))
}

// ───────────── Cuentas anuales: lista y descarga (`/dokumentlista`, `/dokument/{id}`) ─────────────

#[derive(Clone, Debug, PartialEq)]
pub struct DocRef {
    pub id: String,
    /// Fin del ejercicio, "AAAA-MM-DD".
    pub period_end: String,
    /// Fecha de registro, "AAAA-MM-DD" (puede ir vacía).
    pub registered: String,
}

const MAX_DOCUMENT_BYTES: usize = 15 * 1024 * 1024;

async fn auth() -> Result<(Config, String), BvError> {
    let config = load_config().ok_or_else(|| {
        upstream(None, "Bolagsverket: faltan BOLAGSVERKET_CLIENT_ID / BOLAGSVERKET_CLIENT_SECRET / BOLAGSVERKET_BASE_URL (ver .env.example).")
    })?;
    let token = access_token(&config).await?;
    Ok((config, token))
}

/// Convierte una respuesta HTTP no exitosa en un `BvError` con un mensaje útil (sin credenciales).
async fn ensure_ok(res: reqwest::Response, what: &str) -> Result<reqwest::Response, BvError> {
    let s = res.status().as_u16();
    if (200..300).contains(&s) {
        return Ok(res);
    }
    if s == 401 {
        *TOKEN.lock().unwrap() = None;
    }
    let body = res.text().await.unwrap_or_default();
    let detail: String = body.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(300).collect();
    Err(upstream(
        Some(s),
        match s {
            401 => "Bolagsverket: token rechazado (401). Reintenta.".to_string(),
            403 => "Bolagsverket: acceso denegado (403). Comprueba que las credenciales son del entorno correcto.".to_string(),
            429 => "Bolagsverket: límite de 60 peticiones/minuto superado.".to_string(),
            _ => format!("Bolagsverket: error HTTP {s} al consultar {what}. {detail}").trim_end().to_string(),
        },
    ))
}

/// Cuentas anuales digitales (K2/K3/ESEF) disponibles de una organización. Vacío si no presenta en digital.
pub async fn list_documents(input: &str) -> Result<Vec<DocRef>, BvError> {
    let id = normalize_org_number(input)
        .filter(|n| luhn_valid(n))
        .ok_or_else(|| BvError::Invalid(format!("Bolagsverket: \"{input}\" no es un organisationsnummer válido.")))?;
    let (config, token) = auth().await?;
    let res = CLIENT
        .post(format!("{}/dokumentlista", config.base_url))
        .bearer_auth(&token)
        .header("Accept", "application/json")
        .json(&json!({ "identitetsbeteckning": id }))
        .send()
        .await
        .map_err(|e| upstream(None, format!("Bolagsverket: no se pudo consultar la lista de documentos de {id}: {e}")))?;
    let raw: Value = ensure_ok(res, &id)
        .await?
        .json()
        .await
        .map_err(|e| upstream(None, format!("Bolagsverket: respuesta no válida: {e}")))?;
    Ok(parse_document_list(&raw))
}

pub fn parse_document_list(raw: &Value) -> Vec<DocRef> {
    raw.get("dokument")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|d| {
                    Some(DocRef {
                        id: s(d.get("dokumentId"))?,
                        period_end: d.get("rapporteringsperiodTom").and_then(to_iso_date)?,
                        registered: d.get("registreringstidpunkt").and_then(to_iso_date).unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Los identificadores reales tienen la forma `<uuid>_<sufijo>` (42 caracteres, con guion y guion bajo). Se
/// restringe el alfabeto porque el valor se inserta en la ruta de la petición.
pub fn valid_document_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 100 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Descarga un documento (ZIP con el informe iXBRL). Máximo 15 MB.
pub async fn download_document(doc_id: &str) -> Result<Vec<u8>, BvError> {
    if !valid_document_id(doc_id) {
        return Err(BvError::Invalid(format!("Bolagsverket: identificador de documento no válido ({} caracteres).", doc_id.len())));
    }
    let (config, token) = auth().await?;
    let res = CLIENT
        .get(format!("{}/dokument/{doc_id}", config.base_url))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| upstream(None, format!("Bolagsverket: no se pudo descargar el documento: {e}")))?;
    let res = ensure_ok(res, "el documento").await?;
    if res.content_length().is_some_and(|n| n as usize > MAX_DOCUMENT_BYTES) {
        return Err(upstream(None, "Bolagsverket: el documento supera el tamaño máximo admitido (15 MB)."));
    }
    let bytes = res.bytes().await.map_err(|e| upstream(None, format!("Bolagsverket: descarga interrumpida: {e}")))?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(upstream(None, "Bolagsverket: el documento supera el tamaño máximo admitido (15 MB)."));
    }
    Ok(bytes.to_vec())
}

/// Guarda en caché los resultados definitivos (encontrada / no existe); los errores transitorios no.
fn remember(id: String, result: Result<Organisation, BvError>) -> Result<Organisation, BvError> {
    match result {
        Ok(org) => {
            ORG_CACHE.lock().unwrap().insert(id, (Instant::now(), Some(org.clone())));
            Ok(org)
        }
        Err(BvError::NotFound(_)) => {
            ORG_CACHE.lock().unwrap().insert(id.clone(), (Instant::now(), None));
            Err(BvError::NotFound(id))
        }
        Err(e) => Err(e),
    }
}

// ───────────── JSON crudo → modelo ─────────────

fn s(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
}

/// Fechas → "AAAA-MM-DD". La API REAL de producción devuelve texto ISO ("2006-05-10"); la especificación
/// publicada muestra epoch en milisegundos (número o texto numérico, UTC). Se aceptan ambas formas.
pub fn to_iso_date(v: &Value) -> Option<String> {
    if let Some(d) = v.as_str().map(str::trim).and_then(|t| t.get(..10)) {
        let b = d.as_bytes();
        if b[4] == b'-' && b[7] == b'-' && [0, 1, 2, 3, 5, 6, 8, 9].iter().all(|&i| b[i].is_ascii_digit()) {
            return Some(d.to_string());
        }
    }
    let ms = v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)).or_else(|| v.as_str()?.trim().parse().ok())?;
    let z = ms.div_euclid(86_400_000) + 719_468;
    // Algoritmo de Howard Hinnant (días desde 1970 → fecha civil), sin dependencias.
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

/// Cada sección de la respuesta puede traer su propio `fel` (error de esa fuente de datos).
fn fel_types(org: &Value) -> Vec<String> {
    org.as_object()
        .map(|o| o.values().filter_map(|v| v.pointer("/fel/typ").and_then(Value::as_str).map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Nombre de la empresa: el de tipo "Företagsnamn" si existe; si no, el primero de la lista.
fn pick_name(org: &Value) -> Option<String> {
    let list = org.pointer("/organisationsnamn/organisationsnamnLista")?.as_array()?;
    let preferred = list.iter().find(|n| n.pointer("/organisationsnamntyp/kod").and_then(Value::as_str) == Some("FORETAGSNAMN"));
    s(preferred.and_then(|n| n.get("namn"))).or_else(|| list.iter().find_map(|n| s(n.get("namn"))))
}

pub fn map_organisation(raw: &Value, organisationsnummer: &str) -> Result<Organisation, BvError> {
    let Some(org) = raw.pointer("/organisationer/0").filter(|o| o.is_object()) else {
        return Err(BvError::NotFound(organisationsnummer.to_string()));
    };
    let fel = fel_types(org);
    if fel.iter().any(|t| t == "ORGANISATION_FINNS_EJ") {
        return Err(BvError::NotFound(organisationsnummer.to_string()));
    }
    let Some(namn) = pick_name(org) else {
        return Err(if fel.iter().any(|t| t == "OTILLGANGLIG_UPPGIFTSKALLA" || t == "TIMEOUT") {
            upstream(Some(502), "Bolagsverket: una de las fuentes de datos (Bolagsverket/SCB) no estaba disponible. Reintenta más tarde.")
        } else {
            upstream(None, "Bolagsverket: la respuesta no incluye el nombre de la organización.")
        });
    };

    let sni = org
        .pointer("/naringsgrenOrganisation/sni")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| Some((s(x.get("kod"))?, s(x.get("klartext")).unwrap_or_default()))).collect())
        .unwrap_or_default();

    let forfaranden = org
        .pointer("/pagaendeAvvecklingsEllerOmstruktureringsforfarande/pagaendeAvvecklingsEllerOmstruktureringsforfarandeLista")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|f| {
                    let text = s(f.get("klartext")).or_else(|| s(f.get("kod")))?;
                    Some(match f.get("fromDatum").and_then(to_iso_date) {
                        Some(since) => format!("{text} (sedan {since})"),
                        None => text,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let avregistreringsdatum = org.pointer("/avregistreradOrganisation/avregistreringsdatum").and_then(to_iso_date);
    let verksam_nej = org.pointer("/verksamOrganisation/kod").and_then(Value::as_str) == Some("NEJ");
    let adress = |key: &str| s(org.pointer(&format!("/postadressOrganisation/postadress/{key}")));

    Ok(Organisation {
        organisationsnummer: organisationsnummer.to_string(),
        namn,
        organisationsform: s(org.pointer("/organisationsform/klartext"))
            .or_else(|| s(org.pointer("/juridiskForm/klartext")))
            .unwrap_or_else(|| "—".to_string()),
        sni,
        gatuadress: adress("utdelningsadress"),
        postnummer: adress("postnummer"),
        postort: adress("postort"),
        registreringsdatum: org.pointer("/organisationsdatum/registreringsdatum").and_then(to_iso_date).unwrap_or_default(),
        aktiv: avregistreringsdatum.is_none() && !verksam_nej,
        avregistreringsdatum,
        forfaranden,
        // El texto oficial trae saltos de línea y sangrías ("\n       HANDEL MED SKOR."): se colapsan.
        verksamhetsbeskrivning: s(org.pointer("/verksamhetsbeskrivning/beskrivning"))
            .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" ")),
    })
}

#[cfg(test)]
pub(crate) mod fixtures {
    /// Respuesta de `/organisationer` con la forma del ejemplo oficial "aktiebolag" de la especificación.
    pub const AKTIEBOLAG: &str = r#"{"organisationer":[{
      "organisationsidentitet":{"identitetsbeteckning":"5299999994"},
      "namnskyddslopnummer":null,
      "organisationsnamn":{"organisationsnamnLista":[
        {"registreringsdatum":1584230400000,"namn":"Cykelbolaget AB","organisationsnamntyp":{"kod":"FORETAGSNAMN","klartext":"Företagsnamn"},"verksamhetsbeskrivningSarskiltForetagsnamn":null},
        {"registreringsdatum":1710460800000,"namn":"Mopedbolaget AB","organisationsnamntyp":{"kod":"SARSKILT_FORETAGSNAMN","klartext":"Särskilt företagsnamn"},"verksamhetsbeskrivningSarskiltForetagsnamn":"Att bedriva handel med mopeder."},
        {"organisationsnamntyp":{"kod":"FORETAGSNAMN_PA_FRAMMANDE_SPRAK","klartext":"Företagsnamn på främmande språk"},"namn":"Bicycle expert","registreringsdatum":1585699200000,"verksamhetsbeskrivningSarskiltForetagsnamn":null}
      ],"fel":null,"dataproducent":"Bolagsverket"},
      "registreringsland":{"kod":"SE-LAND","klartext":"Sverige"},
      "organisationsform":{"kod":"AB","klartext":"Aktiebolag","fel":null,"dataproducent":"Bolagsverket"},
      "juridiskForm":{"kod":"49","klartext":"Övriga aktiebolag","fel":null,"dataproducent":"SCB"},
      "verksamOrganisation":{"kod":"NEJ","fel":null,"dataproducent":"SCB"},
      "postadressOrganisation":{"postadress":{"postnummer":"12345","utdelningsadress":"Jobbstigen 2","land":"Sverige","coAdress":"C/o Annat företag","postort":"Grönköping"},"fel":null,"dataproducent":"SCB"},
      "verksamhetsbeskrivning":{"fel":null,"dataproducent":"Bolagsverket","beskrivning":"\n       Bedriva handel med cyklar\n   och tillbehör till cyklar"},
      "organisationsdatum":{"registreringsdatum":948585600000,"fel":null,"dataproducent":"Bolagsverket","infortHosScb":949536000000},
      "avregistreradOrganisation":{"avregistreringsdatum":1683244800000,"fel":null,"dataproducent":"Bolagsverket"},
      "pagaendeAvvecklingsEllerOmstruktureringsforfarande":{"pagaendeAvvecklingsEllerOmstruktureringsforfarandeLista":[{"kod":"KK","klartext":"Konkurs","fromDatum":1706227200000},{"kod":"LI","klartext":"Likvidation","fromDatum":1716681600000}],"fel":null,"dataproducent":"Bolagsverket"},
      "naringsgrenOrganisation":{"fel":null,"dataproducent":"Bolagsverket","sni":[{"kod":"47642","klartext":"Specialiserad butikshandel med cyklar"},{"kod":"45400","klartext":"EU-mopeder, reservdelar och tillbehör, handel med"}]}
    }]}"#;

    /// Forma REAL observada en producción (empresa pública): fechas en texto ISO y secciones `null`
    /// cuando no aplican (a diferencia de los ejemplos de la especificación, con epoch en milisegundos).
    pub const PRODUCCION_REAL: &str = r#"{"organisationer":[{
      "organisationsidentitet":{"identitetsbeteckning":"5567037485","typ":{"kod":"ORGANISATIONSNUMMER","klartext":"Organisationsnummer"}},
      "namnskyddslopnummer":null,
      "organisationsnamn":{"organisationsnamnLista":[{"namn":"Spotify AB","organisationsnamntyp":{"kod":"FORETAGSNAMN","klartext":"Företagsnamn"},"registreringsdatum":"2006-05-10","verksamhetsbeskrivningSarskiltForetagsnamn":null}],"fel":null,"dataproducent":"Bolagsverket"},
      "registreringsland":{"kod":"SE-LAND","klartext":"Sverige"},
      "reklamsparr":{"kod":"NEJ","fel":null,"dataproducent":"SCB"},
      "organisationsform":{"kod":"AB","klartext":"Aktiebolag","dataproducent":"Bolagsverket","fel":null},
      "juridiskForm":{"kod":"49","klartext":"Övriga aktiebolag","dataproducent":"SCB","fel":null},
      "verksamOrganisation":{"kod":"JA","dataproducent":"SCB","fel":null},
      "avregistreradOrganisation":null,
      "avregistreringsorsak":null,
      "pagaendeAvvecklingsEllerOmstruktureringsforfarande":null,
      "postadressOrganisation":{"postadress":{"postnummer":"11153","coAdress":null,"land":null,"postort":"STOCKHOLM","utdelningsadress":"Regeringsgatan 19"},"dataproducent":"SCB","fel":null},
      "organisationsdatum":{"registreringsdatum":"2006-05-10","dataproducent":"Bolagsverket","fel":null,"infortHosScb":"2006-05-11"},
      "verksamhetsbeskrivning":{"beskrivning":"Bolaget har till föremål för sin verksamhet att bedriva Internetrelaterade tjänster.","dataproducent":"Bolagsverket","fel":null},
      "naringsgrenOrganisation":{"sni":[{"klartext":"Radiosändning och distribution av ljudinspelningar","kod":"60100"}],"dataproducent":"SCB","fel":null}
    }]}"#;

    pub const FINNS_EJ: &str = r#"{"organisationer":[{"organisationsidentitet":null,
      "organisationsnamn":{"organisationsnamnLista":null,"fel":{"typ":"ORGANISATION_FINNS_EJ","felBeskrivning":"Organisationen finns ej"},"dataproducent":"Bolagsverket"}}]}"#;

    pub const KALLA_OTILLGANGLIG: &str = r#"{"organisationer":[{"organisationsidentitet":null,
      "organisationsnamn":{"organisationsnamnLista":null,"fel":{"typ":"OTILLGANGLIG_UPPGIFTSKALLA","felBeskrivning":"Uppkoppling misslyckades"},"dataproducent":"Bolagsverket"}}]}"#;
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn maps_the_official_aktiebolag_example() {
        let o = map_organisation(&parse(AKTIEBOLAG), "5299999994").unwrap();
        assert_eq!(o.namn, "Cykelbolaget AB", "prefiere el tipo Företagsnamn sobre el nombre especial");
        assert_eq!(o.organisationsform, "Aktiebolag");
        assert_eq!(o.registreringsdatum, "2000-01-23");
        assert_eq!(o.avregistreringsdatum.as_deref(), Some("2023-05-05"));
        assert!(!o.aktiv);
        assert_eq!(o.forfaranden, ["Konkurs (sedan 2024-01-26)", "Likvidation (sedan 2024-05-26)"]);
        assert_eq!(o.sni[0], ("47642".to_string(), "Specialiserad butikshandel med cyklar".to_string()));
        assert_eq!(o.postort.as_deref(), Some("Grönköping"));
        assert_eq!(o.gatuadress.as_deref(), Some("Jobbstigen 2"));
        assert_eq!(o.verksamhetsbeskrivning.as_deref(), Some("Bedriva handel med cyklar och tillbehör till cyklar"));
        assert_eq!(o.formatted_number(), "529999-9994");
    }

    #[test]
    fn maps_the_real_production_shape_with_iso_dates_and_null_sections() {
        let o = map_organisation(&parse(PRODUCCION_REAL), "5567037485").unwrap();
        assert_eq!(o.namn, "Spotify AB");
        assert_eq!(o.registreringsdatum, "2006-05-10", "la API real manda texto ISO, no epoch");
        assert!(o.aktiv, "avregistradOrganisation = null y verksamOrganisation = JA");
        assert_eq!(o.avregistreringsdatum, None);
        assert!(o.forfaranden.is_empty());
        assert_eq!(o.gatuadress.as_deref(), Some("Regeringsgatan 19"));
        assert_eq!(o.postnummer.as_deref(), Some("11153"));
        assert_eq!(o.sni, [("60100".to_string(), "Radiosändning och distribution av ljudinspelningar".to_string())]);
    }

    #[test]
    fn iso_date_strings_are_accepted_and_garbage_is_not() {
        assert_eq!(to_iso_date(&json!("1915-05-05")).as_deref(), Some("1915-05-05"));
        assert_eq!(to_iso_date(&json!("2024-03-15T10:20:30Z")).as_deref(), Some("2024-03-15"));
        assert_eq!(to_iso_date(&json!("inte ett datum")), None);
        assert_eq!(to_iso_date(&json!("åäö-åäö-åäö")), None, "no entra en pánico con texto no ASCII");
    }

    #[test]
    fn active_company_has_no_end_date() {
        let mut j = parse(AKTIEBOLAG);
        j["organisationer"][0]["verksamOrganisation"]["kod"] = json!("JA");
        j["organisationer"][0]["avregistreradOrganisation"] = json!({"avregistreringsdatum": null, "fel": null});
        assert!(map_organisation(&j, "5299999994").unwrap().aktiv);
    }

    #[test]
    fn missing_org_and_unavailable_source_are_distinguished() {
        assert!(matches!(map_organisation(&parse(FINNS_EJ), "5560000001"), Err(BvError::NotFound(_))));
        assert!(matches!(map_organisation(&parse(r#"{"organisationer":[]}"#), "5560000001"), Err(BvError::NotFound(_))));
        assert!(matches!(
            map_organisation(&parse(KALLA_OTILLGANGLIG), "5560000001"),
            Err(BvError::Upstream { status: Some(502), .. })
        ));
    }

    #[test]
    fn epoch_milliseconds_become_iso_dates() {
        let d = |v: Value| to_iso_date(&v);
        assert_eq!(d(json!(948585600000i64)).as_deref(), Some("2000-01-23"));
        assert_eq!(d(json!("1710460800000")).as_deref(), Some("2024-03-15"), "también como texto");
        assert_eq!(d(json!(0)).as_deref(), Some("1970-01-01"));
        assert_eq!(d(json!(951782400000i64)).as_deref(), Some("2000-02-29"), "año bisiesto");
        assert_eq!(d(json!(-86400000i64)).as_deref(), Some("1969-12-31"));
        assert_eq!(d(json!(null)), None);
    }

    #[test]
    fn token_url_follows_the_environment() {
        assert_eq!(default_token_url("https://gw.api.bolagsverket.se/vardefulla-datamangder/v1"), "https://portal.api.bolagsverket.se/oauth2/token");
        assert_eq!(default_token_url("https://gw-accept2.api.bolagsverket.se/vardefulla-datamangder/v1"), "https://portal-accept2.api.bolagsverket.se/oauth2/token");
        assert_eq!(default_token_url("http://127.0.0.1:4010/vardefulla-datamangder/v1"), "http://127.0.0.1:4010/oauth2/token");
    }

    #[test]
    fn document_ids_have_the_real_shape_and_cannot_escape_the_path() {
        assert!(valid_document_id("8a35458e-d80b-4ecf-ba2c-86d263fbf95b_aaaaa"), "forma real: uuid_sufijo");
        assert!(valid_document_id("doc-2025"));
        assert!(!valid_document_id(""));
        assert!(!valid_document_id("../organisationer"));
        assert!(!valid_document_id("a/b"));
        assert!(!valid_document_id("a b"));
        assert!(!valid_document_id(&"a".repeat(101)));
    }

    #[test]
    fn luhn_check_digit() {
        assert!(luhn_valid("5567037485"), "Spotify AB");
        assert!(luhn_valid("5560125790"), "AB Volvo");
        assert!(luhn_valid("5299999994"), "ejemplo oficial");
        assert!(!luhn_valid("5560000009"));
        assert!(!luhn_valid("5567037484"));
    }

    #[test]
    fn only_ten_digit_organisation_numbers_are_accepted() {
        assert_eq!(normalize_org_number("559012-3456").as_deref(), Some("5590123456"));
        assert_eq!(normalize_org_number(" 5590123456 ").as_deref(), Some("5590123456"));
        assert_eq!(normalize_org_number("194009272719"), None, "personnummer de 12 dígitos: rechazado a propósito");
        assert_eq!(normalize_org_number("abc"), None);
        assert_eq!(normalize_org_number("12345"), None);
    }

    /// Contra el entorno REAL de Bolagsverket con las claves de `.env.local`: `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "requiere red y credenciales"]
    async fn live_bolagsverket_lookup() {
        crate::load_dotenv();
        assert!(configured(), "faltan las claves en .env.local");
        let o = get_organisation_by_number("5299999994").await.expect("consulta real");
        println!("{o:#?}");
        assert!(!o.namn.is_empty());
    }
}
