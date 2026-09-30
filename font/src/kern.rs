//! Le crénage : l'espace ajusté entre deux glyphes précis (« T » et « o » se
//! rapprochent dans « To »).
//!
//! Deux sources, comme dans HarfBuzz (le moteur de mise en forme de Chromium) :
//! - la table `GPOS` d'OpenType : les recherches d'ajustement de paires
//!   (`PairPos`, formats 1 et 2) liées à la fonctionnalité `kern` ;
//! - sinon l'ancienne table `kern`, format 0, dans sa version Microsoft ou Apple
//!   (Times et Helvetica sur macOS n'ont que celle-là).
//!
//! Les tables sont gardées telles quelles et lues à chaque paire, bornes
//! vérifiées.

use crate::sfnt::Reader;

/// Le crénage d'une police.
#[derive(Debug, Clone, Default)]
pub(crate) enum Kerning {
    #[default]
    None,
    /// La table `GPOS`, et ses sous-tables `PairPos` : (n° de recherche,
    /// position dans la table).
    Gpos {
        table: Vec<u8>,
        subtables: Vec<(u16, usize)>,
    },
    /// Les paires (gauche, droite, valeur) de la table `kern`, triées.
    Pairs(Vec<(u16, u16, i16)>),
}

impl Kerning {
    pub(crate) fn from_tables(gpos: Option<Reader>, kern: Option<Reader>) -> Kerning {
        if let Some(gpos) = gpos
            && let Some(subtables) = gpos_pair_subtables(gpos)
            && !subtables.is_empty()
        {
            return Kerning::Gpos {
                table: gpos.bytes_all().to_vec(),
                subtables,
            };
        }
        match kern.and_then(kern_pairs) {
            Some(pairs) if !pairs.is_empty() => Kerning::Pairs(pairs),
            _ => Kerning::None,
        }
    }

    /// L'ajustement d'avance du glyphe `left` quand `right` le suit, en unités.
    pub(crate) fn pair(&self, left: u16, right: u16) -> i32 {
        match self {
            Kerning::None => 0,
            Kerning::Pairs(pairs) => pairs
                .binary_search_by_key(&(left, right), |&(l, r, _)| (l, r))
                .map_or(0, |i| pairs[i].2 as i32),
            Kerning::Gpos { table, subtables } => {
                let r = Reader::new(table);
                // Chaque recherche `kern` s'applique à son tour ; dans une
                // recherche, la première sous-table qui concerne la paire gagne.
                let mut total = 0;
                let mut last_lookup = None;
                for &(lookup, offset) in subtables {
                    if last_lookup == Some(lookup) {
                        continue;
                    }
                    if let Some(v) = pair_pos(r, offset, left, right) {
                        total += v;
                        last_lookup = Some(lookup);
                    }
                }
                total
            }
        }
    }
}

// ───────────── GPOS ─────────────

/// Les sous-tables `PairPos` des recherches liées à `kern` :
/// (n° de recherche, position dans la table), dans l'ordre des recherches.
fn gpos_pair_subtables(gpos: Reader) -> Option<Vec<(u16, usize)>> {
    let script_list = gpos.u16(4).ok()? as usize;
    let feature_list = gpos.u16(6).ok()? as usize;
    let lookup_list = gpos.u16(8).ok()? as usize;

    // Les fonctionnalités de l'écriture latine (sinon celle par défaut).
    let features = lang_sys_features(gpos, script_list);
    let feature_count = gpos.u16(feature_list).ok()? as usize;
    let mut lookups: Vec<u16> = Vec::new();
    for f in 0..feature_count {
        if let Some(wanted) = &features
            && !wanted.contains(&(f as u16))
        {
            continue;
        }
        let rec = feature_list + 2 + 6 * f;
        if gpos.bytes(rec, 4).ok()? != b"kern" {
            continue;
        }
        let feature = feature_list + gpos.u16(rec + 4).ok()? as usize;
        let count = gpos.u16(feature + 2).ok()? as usize;
        for i in 0..count {
            lookups.push(gpos.u16(feature + 4 + 2 * i).ok()?);
        }
    }
    lookups.sort_unstable();
    lookups.dedup();

    let mut subtables = Vec::new();
    let lookup_count = gpos.u16(lookup_list).ok()?;
    for l in lookups.into_iter().filter(|&l| l < lookup_count) {
        let lookup = lookup_list + gpos.u16(lookup_list + 2 + 2 * l as usize).ok()? as usize;
        let kind = gpos.u16(lookup).ok()?;
        let count = gpos.u16(lookup + 4).ok()? as usize;
        for s in 0..count {
            let mut sub = lookup + gpos.u16(lookup + 6 + 2 * s).ok()? as usize;
            let mut sub_kind = kind;
            // Type 9 : une « extension » qui pointe (sur 32 bits) vers la vraie
            // sous-table.
            if kind == 9 {
                sub_kind = gpos.u16(sub + 2).ok()?;
                sub += gpos.u32(sub + 4).ok()? as usize;
            }
            if sub_kind == 2 {
                subtables.push((l, sub));
            }
        }
    }
    Some(subtables)
}

/// Les indices de fonctionnalités de l'écriture `latn` (ou `DFLT`), langue par
/// défaut. `None` : pas trouvé, on prendra toutes les fonctionnalités `kern`.
fn lang_sys_features(gpos: Reader, script_list: usize) -> Option<Vec<u16>> {
    let count = gpos.u16(script_list).ok()? as usize;
    let mut script = None;
    for i in 0..count {
        let rec = script_list + 2 + 6 * i;
        let tag = gpos.bytes(rec, 4).ok()?;
        let offset = script_list + gpos.u16(rec + 4).ok()? as usize;
        if tag == b"latn" {
            script = Some(offset);
            break;
        }
        if tag == b"DFLT" {
            script = Some(offset);
        }
    }
    let script = script?;
    let default = gpos.u16(script).ok()? as usize;
    if default == 0 {
        return None;
    }
    let lang_sys = script + default;
    let count = gpos.u16(lang_sys + 4).ok()? as usize;
    (0..count)
        .map(|i| gpos.u16(lang_sys + 6 + 2 * i).ok())
        .collect()
}

/// L'indice d'un glyphe dans une table de couverture.
fn coverage_index(r: Reader, coverage: usize, glyph: u16) -> Option<usize> {
    match r.u16(coverage).ok()? {
        1 => {
            let count = r.u16(coverage + 2).ok()? as usize;
            let (mut lo, mut hi) = (0, count);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let g = r.u16(coverage + 4 + 2 * mid).ok()?;
                match g.cmp(&glyph) {
                    std::cmp::Ordering::Equal => return Some(mid),
                    std::cmp::Ordering::Less => lo = mid + 1,
                    std::cmp::Ordering::Greater => hi = mid,
                }
            }
            None
        }
        2 => {
            let count = r.u16(coverage + 2).ok()? as usize;
            for i in 0..count {
                let rec = coverage + 4 + 6 * i;
                let (start, end) = (r.u16(rec).ok()?, r.u16(rec + 2).ok()?);
                if (start..=end).contains(&glyph) {
                    return Some(r.u16(rec + 4).ok()? as usize + (glyph - start) as usize);
                }
            }
            None
        }
        _ => None,
    }
}

/// La classe d'un glyphe (0 par défaut).
fn class_of(r: Reader, class_def: usize, glyph: u16) -> u16 {
    let class = || -> Option<u16> {
        match r.u16(class_def).ok()? {
            1 => {
                let start = r.u16(class_def + 2).ok()?;
                let count = r.u16(class_def + 4).ok()?;
                let i = glyph.checked_sub(start)?;
                (i < count).then_some(())?;
                r.u16(class_def + 6 + 2 * i as usize).ok()
            }
            2 => {
                let count = r.u16(class_def + 2).ok()? as usize;
                for i in 0..count {
                    let rec = class_def + 4 + 6 * i;
                    let (start, end) = (r.u16(rec).ok()?, r.u16(rec + 2).ok()?);
                    if (start..=end).contains(&glyph) {
                        return r.u16(rec + 4).ok();
                    }
                }
                None
            }
            _ => None,
        }
    };
    class().unwrap_or(0)
}

/// Taille d'un enregistrement de valeurs (2 octets par bit du format).
fn value_size(format: u16) -> usize {
    2 * (format & 0xFF).count_ones() as usize
}

/// L'avance horizontale (`XAdvance`) d'un enregistrement de valeurs, s'il en a.
fn x_advance(r: Reader, record: usize, format: u16) -> Option<i32> {
    if format & 0x0004 == 0 {
        return Some(0);
    }
    // Avant XAdvance : XPlacement (0x1) et YPlacement (0x2), s'ils sont présents.
    let before = 2 * (format & 0x0003).count_ones() as usize;
    Some(r.u16(record + before).ok()? as i16 as i32)
}

/// L'ajustement donné par une sous-table `PairPos`, si elle concerne la paire.
fn pair_pos(r: Reader, sub: usize, left: u16, right: u16) -> Option<i32> {
    let format = r.u16(sub).ok()?;
    let coverage = sub + r.u16(sub + 2).ok()? as usize;
    let (vf1, vf2) = (r.u16(sub + 4).ok()?, r.u16(sub + 6).ok()?);
    let index = coverage_index(r, coverage, left)?;
    match format {
        1 => {
            let count = r.u16(sub + 8).ok()? as usize;
            if index >= count {
                return None;
            }
            let set = sub + r.u16(sub + 10 + 2 * index).ok()? as usize;
            let pairs = r.u16(set).ok()? as usize;
            let record = 2 + value_size(vf1) + value_size(vf2);
            let (mut lo, mut hi) = (0, pairs);
            while lo < hi {
                let mid = (lo + hi) / 2;
                let rec = set + 2 + record * mid;
                let second = r.u16(rec).ok()?;
                match second.cmp(&right) {
                    std::cmp::Ordering::Equal => return x_advance(r, rec + 2, vf1),
                    std::cmp::Ordering::Less => lo = mid + 1,
                    std::cmp::Ordering::Greater => hi = mid,
                }
            }
            None
        }
        2 => {
            let class_def1 = sub + r.u16(sub + 8).ok()? as usize;
            let class_def2 = sub + r.u16(sub + 10).ok()? as usize;
            let class1_count = r.u16(sub + 12).ok()? as usize;
            let class2_count = r.u16(sub + 14).ok()? as usize;
            let (c1, c2) = (
                class_of(r, class_def1, left) as usize,
                class_of(r, class_def2, right) as usize,
            );
            if c1 >= class1_count || c2 >= class2_count {
                return None;
            }
            let record = value_size(vf1) + value_size(vf2);
            let rec = sub + 16 + (c1 * class2_count + c2) * record;
            x_advance(r, rec, vf1)
        }
        _ => None,
    }
}

// ───────────── kern ─────────────

/// Les paires du format 0 de la table `kern` (version Microsoft ou Apple),
/// sous-tables horizontales seulement.
fn kern_pairs(kern: Reader) -> Option<Vec<(u16, u16, i16)>> {
    let mut pairs = Vec::new();
    let apple = kern.u16(0).ok()? == 1;
    let (count, mut offset) = if apple {
        (kern.u32(4).ok()? as usize, 8)
    } else {
        (kern.u16(2).ok()? as usize, 4)
    };
    for _ in 0..count.min(64) {
        let (length, format, horizontal, header) = if apple {
            let coverage = kern.u16(offset + 4).ok()?;
            // Apple : bit 15 vertical, bit 14 perpendiculaire, bit 13 variation.
            (
                kern.u32(offset).ok()? as usize,
                coverage & 0xFF,
                coverage & 0xE000 == 0,
                8,
            )
        } else {
            let coverage = kern.u16(offset + 4).ok()?;
            // Microsoft : bit 0 horizontal, bit 2 perpendiculaire.
            (
                kern.u16(offset + 2).ok()? as usize,
                coverage >> 8,
                coverage & 0x1 != 0 && coverage & 0x4 == 0,
                6,
            )
        };
        if format == 0 && horizontal {
            let body = offset + header;
            let n = kern.u16(body).ok()? as usize;
            for i in 0..n {
                let rec = body + 8 + 6 * i;
                pairs.push((
                    kern.u16(rec).ok()?,
                    kern.u16(rec + 2).ok()?,
                    kern.u16(rec + 4).ok()? as i16,
                ));
            }
        }
        if length == 0 {
            break;
        }
        offset += length;
    }
    pairs.sort_unstable_by_key(|&(l, r, _)| (l, r));
    pairs.dedup_by_key(|p| (p.0, p.1));
    Some(pairs)
}
