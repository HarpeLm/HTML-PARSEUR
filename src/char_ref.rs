//! Outils pour les références de caractères (§13.2.5.72 à §13.2.5.80).

use crate::entities::ENTITIES;

/// Cherche la plus longue entité nommée au début de `input` (sans le '&').
/// Renvoie (nombre d'octets reconnus, caractères de remplacement).
///
/// Exemple : "notit;" -> Some((3, "¬")) car "not" existe mais pas "notit;".
pub fn longest_named_match(input: &[u8]) -> Option<(usize, &'static str)> {
    let mut best = None;

    for i in 1..=input.len() {
        // On compare directement les octets : pas besoin de convertir en &str.
        let name = &input[..i];
        // Première entrée >= name. Si une entité commence par `name`, c'est celle-là.
        let idx = ENTITIES.partition_point(|(n, _)| n.as_bytes() < name);
        match ENTITIES.get(idx) {
            Some((n, value)) if n.as_bytes().starts_with(name) => {
                if n.len() == i {
                    best = Some((i, *value));
                }
            }
            _ => break, // plus aucune entité possible avec ce préfixe
        }
    }
    best
}

/// Transforme le code d'une référence numérique (&#...;) en caractère (§13.2.5.80).
pub fn numeric_reference_char(code: u32) -> char {
    match code {
        0 | 0xD800..=0xDFFF | 0x110000.. => '\u{FFFD}',
        0x80..=0x9F => windows_1252(code).unwrap_or_else(|| char::from_u32(code).unwrap()),
        _ => char::from_u32(code).unwrap(),
    }
}

/// Les pages anciennes écrivaient &#128; en pensant "€" (encodage Windows-1252).
/// La spec corrige ces codes de la plage 0x80-0x9F.
fn windows_1252(code: u32) -> Option<char> {
    let c = match code {
        0x80 => '\u{20AC}',
        0x82 => '\u{201A}',
        0x83 => '\u{0192}',
        0x84 => '\u{201E}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02C6}',
        0x89 => '\u{2030}',
        0x8A => '\u{0160}',
        0x8B => '\u{2039}',
        0x8C => '\u{0152}',
        0x8E => '\u{017D}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02DC}',
        0x99 => '\u{2122}',
        0x9A => '\u{0161}',
        0x9B => '\u{203A}',
        0x9C => '\u{0153}',
        0x9E => '\u{017E}',
        0x9F => '\u{0178}',
        _ => return None,
    };
    Some(c)
}
