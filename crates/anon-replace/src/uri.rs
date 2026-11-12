//! Codage en pourcent minimal pour les cibles de liens.

use std::fmt::Write as _;

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Décode les séquences `%XX`. Renvoie le texte décodé et, pour chaque octet
/// décodé, l'offset d'octet dans `s` (plus une sentinelle de fin). `None` si le
/// résultat n'est pas de l'UTF-8 valide.
pub(crate) fn decode(s: &str) -> Option<(String, Vec<usize>)> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut map = Vec::with_capacity(bytes.len().saturating_add(1));
    let mut i = 0usize;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let hi = bytes.get(i.saturating_add(1)).copied().and_then(hex);
            let lo = bytes.get(i.saturating_add(2)).copied().and_then(hex);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi << 4) | lo);
                map.push(i);
                i = i.saturating_add(3);
                continue;
            }
        }
        out.push(b);
        map.push(i);
        i = i.saturating_add(1);
    }
    map.push(s.len());
    String::from_utf8(out).ok().map(|d| (d, map))
}

/// Code en pourcent ce qui n'est pas permis tel quel dans une URI.
pub(crate) fn encode(s: &str) -> String {
    const KEEP: &str = "-._~!$&'()*+,;=:@/?#";
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || KEEP.contains(c) {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                let _ = write!(out, "%{b:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_maps_offsets() {
        let (d, map) = decode("a%40b%C3%A9").unwrap();
        assert_eq!(d, "a@bé");
        assert_eq!(map, vec![0, 1, 4, 5, 8, 11]);
    }

    #[test]
    fn decode_invalid_utf8_is_none() {
        assert!(decode("%FF").is_none());
        assert_eq!(decode("%zz%4").unwrap().0, "%zz%4");
    }

    #[test]
    fn encode_minimal() {
        assert_eq!(encode("Jean Dupont"), "Jean%20Dupont");
        assert_eq!(encode("jean@example.com"), "jean@example.com");
        assert_eq!(encode("é%"), "%C3%A9%25");
    }
}
