//! Lecture du format de fichier des polices TrueType et OpenType (« sfnt ») :
//! le répertoire des tables, et les tables dont la mise en page a besoin.
//!
//! - `head` : l'unité de la grille (`unitsPerEm`) ;
//! - `hhea`, `OS/2` : les métriques verticales (au-dessus et au-dessous de la
//!   ligne de base, interligne) ;
//! - `maxp`, `hmtx` : la largeur (« avance ») de chaque glyphe ;
//! - `cmap` : quel glyphe pour quel caractère (formats 4 et 12) ;
//! - `name` : le nom de la famille (« Times New Roman ») et du style.
//!
//! Une collection (`.ttc`) contient plusieurs polices qui partagent des tables.
//! Toutes les lectures vérifient les bornes : un fichier abîmé donne une erreur,
//! jamais une panique.

use std::fmt;

/// Une erreur de lecture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontError {
    /// Le fichier n'est pas une police TrueType/OpenType, ou est tronqué.
    Invalid(&'static str),
    /// Une table indispensable manque.
    MissingTable(&'static str),
    /// Pas de police à cet indice dans la collection.
    NoSuchFace(u32),
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FontError::Invalid(what) => write!(f, "police invalide : {what}"),
            FontError::MissingTable(tag) => write!(f, "table « {tag} » absente"),
            FontError::NoSuchFace(i) => write!(f, "pas de police n° {i} dans la collection"),
        }
    }
}

impl std::error::Error for FontError {}

type Result<T> = std::result::Result<T, FontError>;

/// Lecture d'entiers gros-boutistes, bornes vérifiées.
#[derive(Clone, Copy)]
pub(crate) struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Reader { data }
    }

    fn bytes(&self, offset: usize, len: usize) -> Result<&'a [u8]> {
        self.data
            .get(
                offset
                    ..offset
                        .checked_add(len)
                        .ok_or(FontError::Invalid("débordement"))?,
            )
            .ok_or(FontError::Invalid("lecture hors du fichier"))
    }

    pub(crate) fn u16(&self, offset: usize) -> Result<u16> {
        let b = self.bytes(offset, 2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    pub(crate) fn i16(&self, offset: usize) -> Result<i16> {
        Ok(self.u16(offset)? as i16)
    }

    pub(crate) fn u32(&self, offset: usize) -> Result<u32> {
        let b = self.bytes(offset, 4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Une sous-partie `offset..offset+len`.
    pub(crate) fn slice(&self, offset: usize, len: usize) -> Result<Reader<'a>> {
        Ok(Reader::new(self.bytes(offset, len)?))
    }

    /// Tout depuis `offset`.
    pub(crate) fn from(&self, offset: usize) -> Result<Reader<'a>> {
        self.data
            .get(offset..)
            .map(Reader::new)
            .ok_or(FontError::Invalid("lecture hors du fichier"))
    }
}

/// Le nombre de polices d'un fichier (1, ou plus pour une collection `.ttc`).
pub fn face_count(data: &[u8]) -> Result<u32> {
    let r = Reader::new(data);
    if r.bytes(0, 4)? == b"ttcf" {
        r.u32(8)
    } else {
        Ok(1)
    }
}

/// Le répertoire des tables d'une police : étiquette -> (début, longueur).
pub(crate) struct Tables<'a> {
    file: Reader<'a>,
    records: Vec<([u8; 4], u32, u32)>,
}

impl<'a> Tables<'a> {
    pub(crate) fn parse(data: &'a [u8], index: u32) -> Result<Tables<'a>> {
        let file = Reader::new(data);
        let offset = if file.bytes(0, 4)? == b"ttcf" {
            let count = file.u32(8)?;
            if index >= count {
                return Err(FontError::NoSuchFace(index));
            }
            file.u32(12 + 4 * index as usize)? as usize
        } else if index == 0 {
            0
        } else {
            return Err(FontError::NoSuchFace(index));
        };
        let version = file.bytes(offset, 4)?;
        if !matches!(version, [0, 1, 0, 0] | b"true" | b"OTTO" | b"typ1") {
            return Err(FontError::Invalid("signature inconnue"));
        }
        let count = file.u16(offset + 4)? as usize;
        let mut records = Vec::with_capacity(count);
        for i in 0..count {
            let rec = offset + 12 + 16 * i;
            let tag = file.bytes(rec, 4)?;
            records.push((
                [tag[0], tag[1], tag[2], tag[3]],
                file.u32(rec + 8)?,
                file.u32(rec + 12)?,
            ));
        }
        Ok(Tables { file, records })
    }

    pub(crate) fn get(&self, tag: &[u8; 4]) -> Option<Reader<'a>> {
        let (_, offset, len) = self.records.iter().find(|(t, _, _)| t == tag)?;
        self.file.slice(*offset as usize, *len as usize).ok()
    }

    pub(crate) fn require(&self, tag: &'static str) -> Result<Reader<'a>> {
        let bytes: [u8; 4] = tag.as_bytes().try_into().expect("étiquette de 4 octets");
        self.get(&bytes).ok_or(FontError::MissingTable(tag))
    }
}

/// La table `cmap` : caractère -> glyphe.
#[derive(Debug, Clone, Default)]
pub(crate) struct CharMap {
    /// Des plages (premier caractère, dernier, glyphe du premier) triées, pour
    /// les caractères dont les glyphes se suivent (format 12, et format 4 sans
    /// tableau de glyphes).
    ranges: Vec<(u32, u32, u32)>,
    /// Les caractères isolés (format 4 avec tableau de glyphes).
    single: Vec<(u32, u16)>,
}

impl CharMap {
    pub(crate) fn parse(cmap: Reader) -> Result<CharMap> {
        let count = cmap.u16(2)? as usize;
        // On préfère une table Unicode complète (format 12), puis BMP (format 4).
        let mut best: Option<(u8, usize)> = None;
        for i in 0..count {
            let rec = 4 + 8 * i;
            let (platform, encoding) = (cmap.u16(rec)?, cmap.u16(rec + 2)?);
            let offset = cmap.u32(rec + 4)? as usize;
            let format = cmap.u16(offset)?;
            let unicode = platform == 0 || (platform == 3 && matches!(encoding, 1 | 10));
            let score = match (unicode, format) {
                (true, 12) => 2,
                (true, 4) => 1,
                _ => continue,
            };
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, offset));
            }
        }
        let Some((_, offset)) = best else {
            return Err(FontError::Invalid("pas de table cmap Unicode"));
        };
        let sub = cmap.from(offset)?;
        let mut map = CharMap::default();
        match sub.u16(0)? {
            4 => {
                let seg_count = sub.u16(6)? as usize / 2;
                let ends = 14;
                let starts = ends + 2 * seg_count + 2;
                let deltas = starts + 2 * seg_count;
                let range_offsets = deltas + 2 * seg_count;
                for s in 0..seg_count {
                    let end = sub.u16(ends + 2 * s)? as u32;
                    let start = sub.u16(starts + 2 * s)? as u32;
                    let delta = sub.u16(deltas + 2 * s)?;
                    let ro_pos = range_offsets + 2 * s;
                    let range_offset = sub.u16(ro_pos)? as usize;
                    if start > end || start == 0xFFFF {
                        continue;
                    }
                    if range_offset == 0 {
                        let first = (start as u16).wrapping_add(delta) as u32;
                        // Les glyphes ne se suivent que tant que l'addition ne
                        // « fait pas le tour » de 65 536.
                        if first + (end - start) <= 0xFFFF {
                            map.ranges.push((start, end, first));
                        } else {
                            for c in start..=end {
                                map.single.push((c, (c as u16).wrapping_add(delta)));
                            }
                        }
                    } else {
                        for c in start..=end {
                            let pos = ro_pos + range_offset + 2 * (c - start) as usize;
                            let glyph = sub.u16(pos)?;
                            if glyph != 0 {
                                map.single.push((c, glyph.wrapping_add(delta)));
                            }
                        }
                    }
                }
            }
            12 => {
                let groups = sub.u32(12)? as usize;
                for g in 0..groups {
                    let rec = 16 + 12 * g;
                    let (start, end, glyph) = (sub.u32(rec)?, sub.u32(rec + 4)?, sub.u32(rec + 8)?);
                    if start <= end {
                        map.ranges.push((start, end, glyph));
                    }
                }
            }
            _ => return Err(FontError::Invalid("format de cmap non géré")),
        }
        map.ranges.sort_unstable();
        map.single.sort_unstable();
        Ok(map)
    }

    /// Le glyphe d'un caractère (0 : glyphe « absent », `.notdef`).
    pub(crate) fn glyph(&self, c: char) -> u16 {
        let c = c as u32;
        if let Ok(i) = self.single.binary_search_by_key(&c, |&(ch, _)| ch) {
            return self.single[i].1;
        }
        let i = self.ranges.partition_point(|&(start, _, _)| start <= c);
        if i > 0 {
            let (start, end, glyph) = self.ranges[i - 1];
            if c <= end {
                return (glyph + (c - start)).min(u16::MAX as u32) as u16;
            }
        }
        0
    }
}

/// Un texte de la table `name`.
pub(crate) fn name(table: Reader, wanted: u16) -> Option<String> {
    let count = table.u16(2).ok()? as usize;
    let strings = table.u16(4).ok()? as usize;
    let mut fallback = None;
    for i in 0..count {
        let rec = 6 + 12 * i;
        let (platform, encoding, language, id) = (
            table.u16(rec).ok()?,
            table.u16(rec + 2).ok()?,
            table.u16(rec + 4).ok()?,
            table.u16(rec + 6).ok()?,
        );
        if id != wanted {
            continue;
        }
        let (len, offset) = (
            table.u16(rec + 8).ok()? as usize,
            table.u16(rec + 10).ok()? as usize,
        );
        let bytes = table.slice(strings + offset, len).ok()?.data;
        match (platform, encoding) {
            // Unicode ou Windows : UTF-16 gros-boutiste. L'anglais d'abord.
            (0, _) | (3, 0 | 1 | 10) => {
                let units: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|p| u16::from_be_bytes([p[0], p[1]]))
                    .collect();
                let text = String::from_utf16_lossy(&units);
                if platform == 0 || language == 0x0409 {
                    return Some(text);
                }
                fallback.get_or_insert(text);
            }
            // Macintosh Roman : l'ASCII suffit pour les noms de familles.
            (1, 0) => {
                fallback.get_or_insert(bytes.iter().map(|&b| b as char).collect());
            }
            _ => {}
        }
    }
    fallback
}
