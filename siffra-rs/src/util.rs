//! Utilidades pequeñas sin dependencias de dominio: tiempo, aleatoriedad, hashes y URL.

use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

pub fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Fecha civil (año, mes, día) a partir de días desde 1970-01-01 (algoritmo de Howard Hinnant).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// "AAAA-MM-DDTHH:MM:SSZ" (UTC). Ordena igual que el tiempo, así que sirve para comparar en SQL.
pub fn iso_from_unix(secs: i64) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn now_iso() -> String {
    iso_from_unix(now_unix())
}

pub fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    getrandom::getrandom(&mut buf).expect("el sistema no pudo dar bytes aleatorios");
    buf
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn random_hex(n_bytes: usize) -> String {
    hex(&random_bytes(n_bytes))
}

pub fn sha256_hex(s: &str) -> String {
    hex(&Sha256::digest(s.as_bytes()))
}

/// Comparación en tiempo constante (para tokens CSRF y similares).
pub fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Contraseña temporal legible (sin caracteres ambiguos como 0/O o 1/l/I), con muestreo sin sesgo.
pub fn gen_password(len: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789"; // 54
    let mut out = String::with_capacity(len);
    while out.len() < len {
        for b in random_bytes(len * 2) {
            if b < 216 && out.len() < len {
                out.push(ALPHABET[(b % 54) as usize] as char);
            }
        }
    }
    out
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Decodifica un valor de query (`%XX` y `+`). Ignora secuencias inválidas.
pub fn urldecode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() && b.get(i + 1).zip(b.get(i + 2)).is_some_and(|(h, l)| h.is_ascii_hexdigit() && l.is_ascii_hexdigit()) => {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap_or(b'?'));
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Valor de un parámetro de una query string cruda.
pub fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (k == key).then(|| urldecode(v))
    })
}

/// La misma query sin el parámetro `key` (para construir enlaces que conservan el resto).
pub fn query_without(query: &str, key: &str) -> String {
    query.split('&').filter(|kv| !kv.is_empty() && kv.split('=').next() != Some(key)).collect::<Vec<_>>().join("&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_dates() {
        assert_eq!(iso_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_from_unix(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso_from_unix(1_790_000_000 + 3_725), iso_from_unix(1_790_003_725));
        assert!(iso_from_unix(1_790_003_725).ends_with('Z'));
        assert_eq!(iso_from_unix(-1), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn tokens_and_passwords() {
        assert_eq!(random_hex(16).len(), 32);
        assert_ne!(random_hex(16), random_hex(16));
        let p = gen_password(16);
        assert_eq!(p.len(), 16);
        assert!(p.chars().all(|c| c.is_ascii_alphanumeric() && !"0O1lI".contains(c)));
        assert_eq!(sha256_hex("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn constant_time_equality() {
        assert!(ct_eq("abc", "abc"));
        assert!(!ct_eq("abc", "abd"));
        assert!(!ct_eq("abc", "ab"));
    }

    #[test]
    fn url_helpers_round_trip() {
        assert_eq!(urlencode("Göteborg & Co"), "G%C3%B6teborg%20%26%20Co");
        assert_eq!(urldecode("G%C3%B6teborg+%26+Co"), "Göteborg & Co");
        assert_eq!(urldecode("100%"), "100%");
        assert_eq!(query_param("q=a%20b&lang=en", "lang").as_deref(), Some("en"));
        assert_eq!(query_param("q=a", "lang"), None);
        assert_eq!(query_without("q=a&lang=en&sort=x", "lang"), "q=a&sort=x");
    }
}
