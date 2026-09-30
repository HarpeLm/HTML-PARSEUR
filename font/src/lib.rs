//! # lumen-font
//!
//! Lecture des polices TrueType et OpenType (`.ttf`, `.otf`, collections `.ttc`)
//! et mesure du texte. C'est une brique de Lumen, un navigateur web écrit de
//! zéro, sans dépendance.
//!
//! - [`Font`] : une police chargée ; ses métriques et la largeur des glyphes ;
//! - [`FontDatabase`] : les polices installées, et le choix d'une police pour
//!   une famille CSS (`"Times New Roman"`, `serif`...), comme un navigateur.
//!
//! Les largeurs sont vérifiées contre celles que mesure Chromium sur les mêmes
//! polices (tests/oracle_mesures.rs).
//!
//! ```no_run
//! use lumen_font::FontDatabase;
//!
//! let db = FontDatabase::system();
//! let font = db.query("Arial", 400, false).expect("Arial installée");
//! // « Hello » en Arial à 16px : 36,4609375 px, comme dans Chromium.
//! assert_eq!(font.text_width("Hello", 16.0), 36.4609375);
//! ```

#![warn(missing_docs)]

mod database;
mod kern;
mod sfnt;

pub use database::{FaceInfo, FontDatabase};
pub use sfnt::{FontError, face_count};

use kern::Kerning;
use sfnt::{CharMap, Reader, Tables};

/// Les métriques verticales d'une police, en unités de la police.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VerticalMetrics {
    /// Hauteur au-dessus de la ligne de base (positive).
    pub ascender: i16,
    /// Profondeur au-dessous de la ligne de base (négative, comme dans le fichier).
    pub descender: i16,
    /// Espace supplémentaire entre deux lignes.
    pub line_gap: i16,
}

/// Une police chargée.
#[derive(Debug, Clone)]
pub struct Font {
    /// La famille (« Times New Roman »).
    pub family: String,
    /// Le style (« Regular », « Bold Italic »...).
    pub subfamily: String,
    /// La graisse (100 à 900 ; 400 : normale, 700 : grasse).
    pub weight: u16,
    /// Italique ou oblique.
    pub italic: bool,
    /// La taille de la grille de dessin (souvent 1000 ou 2048).
    pub units_per_em: u16,
    /// Métriques de la table `hhea` (celles qu'utilisent macOS et Chromium sur Mac).
    pub hhea: VerticalMetrics,
    /// Métriques « typographiques » de la table `OS/2`, si elle existe.
    pub typo: Option<VerticalMetrics>,
    /// `usWinAscent` et `usWinDescent` de la table `OS/2` (Windows).
    pub win: Option<(u16, u16)>,
    advances: Vec<u16>,
    cmap: CharMap,
    kerning: Kerning,
}

impl Font {
    /// Lit la police n° `index` d'un fichier (0 pour un fichier simple).
    pub fn parse(data: &[u8], index: u32) -> Result<Font, FontError> {
        let tables = Tables::parse(data, index)?;
        let head = tables.require("head")?;
        let hhea = tables.require("hhea")?;
        let maxp = tables.require("maxp")?;
        let hmtx = tables.require("hmtx")?;

        let units_per_em = head.u16(18)?;
        if units_per_em == 0 {
            return Err(FontError::Invalid("unitsPerEm nul"));
        }
        let mac_style = head.u16(44)?;
        let metrics = VerticalMetrics {
            ascender: hhea.i16(4)?,
            descender: hhea.i16(6)?,
            line_gap: hhea.i16(8)?,
        };
        let long_metrics = hhea.u16(34)? as usize;
        let glyphs = maxp.u16(4)? as usize;
        let mut advances = Vec::with_capacity(glyphs);
        let mut last = 0;
        for g in 0..glyphs {
            if g < long_metrics {
                last = hmtx.u16(4 * g)?;
            }
            // Au-delà de `numberOfHMetrics`, les glyphes gardent la dernière avance.
            advances.push(last);
        }

        let os2 = tables.get(b"OS/2");
        let read_os2 = |os2: Reader| -> Result<_, FontError> {
            let weight = os2.u16(4)?;
            let selection = os2.u16(62)?;
            let typo = VerticalMetrics {
                ascender: os2.i16(68)?,
                descender: os2.i16(70)?,
                line_gap: os2.i16(72)?,
            };
            let win = (os2.u16(74)?, os2.u16(76)?);
            Ok((weight, selection, typo, win))
        };
        let (weight, italic, typo, win) = match os2.map(read_os2) {
            Some(Ok((weight, selection, typo, win))) => (
                weight,
                selection & 1 != 0 || selection & 0x200 != 0,
                Some(typo),
                Some(win),
            ),
            _ => (
                if mac_style & 1 != 0 { 700 } else { 400 },
                mac_style & 2 != 0,
                None,
                None,
            ),
        };

        let names = tables.get(b"name");
        let text = |id| names.and_then(|n| sfnt::name(n, id));
        Ok(Font {
            family: text(16).or_else(|| text(1)).unwrap_or_default(),
            subfamily: text(17).or_else(|| text(2)).unwrap_or_default(),
            weight,
            italic,
            units_per_em,
            hhea: metrics,
            typo,
            win,
            advances,
            cmap: CharMap::parse(tables.require("cmap")?)?,
            kerning: Kerning::from_tables(tables.get(b"GPOS"), tables.get(b"kern")),
        })
    }

    /// Une police vide (aucun glyphe, métriques nulles) : le dernier recours
    /// quand aucune police n'est installée.
    pub fn empty() -> Font {
        Font {
            family: String::new(),
            subfamily: String::new(),
            weight: 400,
            italic: false,
            units_per_em: 1000,
            hhea: VerticalMetrics::default(),
            typo: None,
            win: None,
            advances: Vec::new(),
            cmap: CharMap::default(),
            kerning: Kerning::None,
        }
    }

    /// Le glyphe qui dessine `c` (0 : absent de la police).
    pub fn glyph(&self, c: char) -> u16 {
        self.cmap.glyph(c)
    }

    /// La police a-t-elle un glyphe pour `c` ?
    pub fn has_glyph(&self, c: char) -> bool {
        self.glyph(c) != 0
    }

    /// L'avance d'un glyphe, en unités de la police.
    pub fn advance(&self, glyph: u16) -> u16 {
        self.advances.get(glyph as usize).copied().unwrap_or(0)
    }

    /// La largeur d'un texte à la taille `size` (px), sans crénage ni ligatures :
    /// la somme des avances de ses glyphes.
    pub fn text_width(&self, text: &str, size: f64) -> f64 {
        let units: u64 = text
            .chars()
            .map(|c| self.advance(self.glyph(c)) as u64)
            .sum();
        units as f64 * size / self.units_per_em as f64
    }

    /// Le crénage entre deux glyphes (en unités) : ce qu'il faut ajouter à
    /// l'avance de `left` quand `right` le suit.
    pub fn kerning(&self, left: u16, right: u16) -> i32 {
        self.kerning.pair(left, right)
    }

    /// L'avance de chaque caractère d'un texte, crénage compris (en unités) :
    /// le crénage d'une paire s'ajoute au premier des deux, comme dans HarfBuzz.
    pub fn kerned_advances(&self, text: &str) -> Vec<i32> {
        let glyphs: Vec<u16> = text.chars().map(|c| self.glyph(c)).collect();
        glyphs
            .iter()
            .enumerate()
            .map(|(i, &g)| {
                let kern = glyphs.get(i + 1).map_or(0, |&next| self.kerning(g, next));
                self.advance(g) as i32 + kern
            })
            .collect()
    }

    /// La largeur d'un texte à la taille `size` (px), crénage compris.
    pub fn kerned_width(&self, text: &str, size: f64) -> f64 {
        let units: i64 = self.kerned_advances(text).iter().map(|&a| a as i64).sum();
        units as f64 * size / self.units_per_em as f64
    }

    /// Convertit des unités de la police en px à la taille `size`.
    pub fn to_px(&self, units: f64, size: f64) -> f64 {
        units * size / self.units_per_em as f64
    }

    /// Les métriques de ligne à la taille `size`, calculées comme Chromium sur
    /// macOS : celles de `hhea`, arrondies au pixel ; et pour Times, Helvetica
    /// et Courier, 15 % de la hauteur ajoutés au-dessus de la ligne de base
    /// (pour ressembler à leurs équivalents Windows, qui font foi sur le web).
    pub fn line_metrics(&self, size: f64) -> LineMetrics {
        let px = |units: i16| self.to_px(units as f64, size).round();
        let mut ascent = px(self.hhea.ascender);
        let descent = px(-self.hhea.descender);
        if matches!(self.family.as_str(), "Times" | "Helvetica" | "Courier") {
            ascent += ((ascent + descent) * 0.15 + 0.5).floor();
        }
        LineMetrics {
            ascent,
            descent,
            line_gap: px(self.hhea.line_gap),
        }
    }
}

/// Les métriques d'une ligne de texte, en px.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LineMetrics {
    /// Hauteur au-dessus de la ligne de base.
    pub ascent: f64,
    /// Profondeur au-dessous de la ligne de base (positive).
    pub descent: f64,
    /// Espace supplémentaire entre deux lignes.
    pub line_gap: f64,
}
