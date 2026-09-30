//! Les polices installées, et le choix d'une police pour une famille CSS.
//!
//! Au démarrage, on ne lit que le répertoire des tables et les petites tables
//! `name`, `OS/2` et `head` de chaque fichier (certaines polices système pèsent
//! des centaines de Mo). Une police n'est chargée en entier qu'à sa première
//! utilisation, puis gardée.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::Font;
use crate::sfnt::{self, Reader};

/// Ce qu'on sait d'une police installée sans l'avoir chargée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceInfo {
    /// Le fichier.
    pub path: PathBuf,
    /// L'indice dans le fichier (collections `.ttc`).
    pub index: u32,
    /// La famille (« Times New Roman »).
    pub family: String,
    /// Le style (« Bold »...).
    pub subfamily: String,
    /// La graisse (100 à 900).
    pub weight: u16,
    /// Italique ou oblique.
    pub italic: bool,
}

/// Les polices déjà chargées (ou dont le chargement a échoué), par fichier et indice.
type Loaded = HashMap<(PathBuf, u32), Option<Rc<Font>>>;

/// Les polices disponibles.
#[derive(Debug, Default)]
pub struct FontDatabase {
    faces: Vec<FaceInfo>,
    loaded: RefCell<Loaded>,
}

/// Les polices génériques de CSS, comme les choisit Chromium sur macOS.
fn generic(family: &str) -> Option<&'static str> {
    Some(match family {
        "serif" => "Times",
        "sans-serif" => "Helvetica",
        "monospace" => "Menlo",
        "cursive" => "Apple Chancery",
        "fantasy" => "Papyrus",
        "system-ui" | "-apple-system" => "System Font",
        _ => return None,
    })
}

fn read_at(file: &mut File, offset: u64, len: usize) -> Option<Vec<u8>> {
    let mut buf = vec![0; len];
    file.seek(SeekFrom::Start(offset)).ok()?;
    file.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// Lit les informations d'une police d'un fichier sans le charger en entier.
fn face_info(file: &mut File, path: &Path, index: u32, dir_offset: u64) -> Option<FaceInfo> {
    let header = read_at(file, dir_offset, 12)?;
    let count = u16::from_be_bytes([header[4], header[5]]) as usize;
    let dir = read_at(file, dir_offset + 12, 16 * count)?;
    let table = |tag: &[u8; 4], file: &mut File| -> Option<Vec<u8>> {
        let rec = dir.chunks_exact(16).find(|r| &r[..4] == tag)?;
        let offset = u32::from_be_bytes([rec[8], rec[9], rec[10], rec[11]]) as u64;
        let len = u32::from_be_bytes([rec[12], rec[13], rec[14], rec[15]]) as usize;
        read_at(file, offset, len.min(1 << 20))
    };
    let name = table(b"name", file)?;
    let text = |id| sfnt::name(Reader::new(&name), id);
    let family = text(16).or_else(|| text(1))?;
    let subfamily = text(17).or_else(|| text(2)).unwrap_or_default();
    let (weight, italic) = match table(b"OS/2", file) {
        Some(os2) if os2.len() >= 64 => {
            let r = Reader::new(&os2);
            let selection = r.u16(62).ok()?;
            (r.u16(4).ok()?, selection & 1 != 0 || selection & 0x200 != 0)
        }
        _ => {
            let head = table(b"head", file)?;
            let style = Reader::new(&head).u16(44).ok()?;
            (if style & 1 != 0 { 700 } else { 400 }, style & 2 != 0)
        }
    };
    Some(FaceInfo {
        path: path.to_path_buf(),
        index,
        family,
        subfamily,
        weight,
        italic,
    })
}

/// Toutes les polices d'un fichier.
fn faces_of(path: &Path) -> Vec<FaceInfo> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let Some(header) = read_at(&mut file, 0, 12) else {
        return Vec::new();
    };
    if &header[..4] == b"ttcf" {
        let count = u32::from_be_bytes([header[8], header[9], header[10], header[11]]);
        let Some(offsets) = read_at(&mut file, 12, 4 * count.min(256) as usize) else {
            return Vec::new();
        };
        offsets
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(i, o)| {
                let offset = u32::from_be_bytes([o[0], o[1], o[2], o[3]]) as u64;
                face_info(&mut file, path, i as u32, offset)
            })
            .collect()
    } else {
        face_info(&mut file, path, 0, 0).into_iter().collect()
    }
}

impl FontDatabase {
    /// Les polices installées sur le système (macOS, Linux).
    pub fn system() -> FontDatabase {
        let mut dirs: Vec<PathBuf> = vec![
            "/System/Library/Fonts".into(),
            "/Library/Fonts".into(),
            "/usr/share/fonts".into(),
            "/usr/local/share/fonts".into(),
        ];
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(Path::new(&home).join("Library/Fonts"));
            dirs.push(Path::new(&home).join(".fonts"));
        }
        FontDatabase::from_dirs(&dirs)
    }

    /// Les polices de ces dossiers (et de leurs sous-dossiers).
    pub fn from_dirs(dirs: &[PathBuf]) -> FontDatabase {
        let mut faces = Vec::new();
        let mut stack: Vec<PathBuf> = dirs.to_vec();
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut paths: Vec<PathBuf> = entries.filter_map(|e| Some(e.ok()?.path())).collect();
            paths.sort();
            for path in paths {
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                    matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc")
                }) {
                    faces.extend(faces_of(&path));
                }
            }
        }
        FontDatabase {
            faces,
            loaded: RefCell::default(),
        }
    }

    /// Les polices connues.
    pub fn faces(&self) -> &[FaceInfo] {
        &self.faces
    }

    /// La police d'une famille la plus proche de la graisse et du style
    /// demandés (règles de choix de CSS Fonts, simplifiées) ; les familles
    /// génériques (`serif`...) sont traduites comme dans Chromium sur macOS.
    pub fn query(&self, family: &str, weight: u16, italic: bool) -> Option<Rc<Font>> {
        let family = generic(&family.to_ascii_lowercase()).unwrap_or(family);
        let candidates: Vec<&FaceInfo> = self
            .faces
            .iter()
            .filter(|f| f.family.eq_ignore_ascii_case(family))
            .collect();
        let best = candidates.iter().min_by_key(|f| {
            // Le style d'abord, puis la graisse (vers le bas si on demande peu,
            // vers le haut si on demande beaucoup, comme CSS Fonts).
            let style_miss = (f.italic != italic) as u32;
            let distance = if weight <= 500 {
                if f.weight <= weight {
                    (weight - f.weight) as u32
                } else {
                    1000 + (f.weight - weight) as u32
                }
            } else if f.weight >= weight {
                (f.weight - weight) as u32
            } else {
                1000 + (weight - f.weight) as u32
            };
            (style_miss, distance)
        })?;
        self.load(&best.path, best.index)
    }

    /// Charge (une seule fois) la police n° `index` d'un fichier.
    pub fn load(&self, path: &Path, index: u32) -> Option<Rc<Font>> {
        let key = (path.to_path_buf(), index);
        if let Some(font) = self.loaded.borrow().get(&key) {
            return font.clone();
        }
        let font = std::fs::read(path)
            .ok()
            .and_then(|data| Font::parse(&data, index).ok())
            .map(Rc::new);
        self.loaded.borrow_mut().insert(key, font.clone());
        font
    }
}
