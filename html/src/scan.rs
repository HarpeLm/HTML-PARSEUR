//! Recherche rapide du premier octet "spécial" dans un texte.
//!
//! C'est la boucle la plus chaude du tokenizer : dans l'état Data, on cherche le
//! prochain `<`, `&`, `\r` ou `\0` ; dans un nom de balise, le prochain espace,
//! `/` ou `>`, etc. Sur ARM64 (Apple M1..M5) on utilise les instructions NEON
//! pour examiner 16 octets à la fois ; ailleurs, une simple boucle.

/// Position du premier octet de `haystack` qui fait partie de `stops`,
/// ou `haystack.len()` s'il n'y en a aucun.
#[inline]
pub fn find_first_of<const N: usize>(haystack: &[u8], stops: &[u8; N]) -> usize {
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY : NEON est obligatoire sur toutes les puces ARM64 (aarch64).
        unsafe { find_first_of_neon(haystack, stops) }
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        find_first_of_scalar(haystack, stops)
    }
}

/// Version simple, octet par octet. Sert de référence pour les tests et à finir
/// les derniers octets (moins de 16) après la boucle SIMD.
#[inline]
pub fn find_first_of_scalar<const N: usize>(haystack: &[u8], stops: &[u8; N]) -> usize {
    haystack
        .iter()
        .position(|b| stops.contains(b))
        .unwrap_or(haystack.len())
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
fn find_first_of_neon<const N: usize>(haystack: &[u8], stops: &[u8; N]) -> usize {
    use std::arch::aarch64::*;

    // Un registre par octet d'arrêt, avec cet octet répété 16 fois.
    let mut needles = [vdupq_n_u8(0); N];
    for k in 0..N {
        needles[k] = vdupq_n_u8(stops[k]);
    }

    let mut i = 0;
    while i + 16 <= haystack.len() {
        // SAFETY : i + 16 <= len, donc les 16 octets lus sont dans le slice.
        let chunk = unsafe { vld1q_u8(haystack.as_ptr().add(i)) };

        // Pour chaque octet du bloc : 0xFF s'il est égal à un octet d'arrêt, 0x00 sinon.
        let mut hits = vdupq_n_u8(0);
        for needle in &needles {
            hits = vorrq_u8(hits, vceqq_u8(chunk, *needle));
        }

        // NEON n'a pas d'instruction "movemask" comme x86. L'astuce classique :
        // `shrn` décale chaque paire d'octets de 4 bits et garde la moitié basse,
        // ce qui résume les 16 octets en 64 bits (4 bits par octet d'origine).
        let mask = vget_lane_u64::<0>(vreinterpret_u64_u8(vshrn_n_u16::<4>(vreinterpretq_u16_u8(
            hits,
        ))));

        if mask != 0 {
            // Le premier bit à 1 donne la position : 4 bits par octet.
            return i + (mask.trailing_zeros() / 4) as usize;
        }
        i += 16;
    }

    // Moins de 16 octets restants : on finit à la main.
    i + find_first_of_scalar(&haystack[i..], stops)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STOPS: &[u8; 4] = b"<&\r\0";

    #[test]
    fn identique_a_la_version_simple() {
        // Pour chaque longueur de 0 à 100 et chaque position de l'octet spécial,
        // la version rapide doit donner exactement le même résultat.
        for len in 0..100 {
            let mut text = vec![b'a'; len];
            assert_eq!(find_first_of(&text, STOPS), len, "aucun arrêt, len={len}");
            for pos in 0..len {
                for &stop in STOPS {
                    text[pos] = stop;
                    assert_eq!(
                        find_first_of(&text, STOPS),
                        find_first_of_scalar(&text, STOPS),
                        "len={len} pos={pos} stop={stop}"
                    );
                    text[pos] = b'a';
                }
            }
        }
    }

    #[test]
    fn premier_de_plusieurs() {
        let text = b"0123456789abcdefghij&klm<nop";
        assert_eq!(find_first_of(text, STOPS), 20);
    }

    #[test]
    fn utf8_non_ascii() {
        // Les octets d'un caractère multi-octets (>= 0x80) ne sont jamais des arrêts.
        let text = "é€😀 caractères accentués très longs pour dépasser 16 octets <".as_bytes();
        assert_eq!(find_first_of(text, STOPS), text.len() - 1);
    }
}
