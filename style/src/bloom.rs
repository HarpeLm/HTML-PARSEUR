//! Un filtre de Bloom des ancêtres, pour rejeter vite les sélecteurs à
//! combinateur descendant ou enfant (`.article .note`), comme Stylo, Blink et
//! WebKit.
//!
//! Pendant le parcours de l'arbre, le filtre contient l'empreinte (hash) des
//! balises, ids et classes de tous les ancêtres de l'élément courant. Un
//! sélecteur connaît les balises, ids et classes que ses ancêtres doivent
//! avoir ; si l'une d'elles est absente du filtre, il ne peut pas correspondre.
//! Le filtre peut se tromper dans un seul sens (« peut-être présent » à tort) :
//! on fait alors le test complet. Il ne rejette jamais un sélecteur valable.

use lumen_css::selectors::{Combinator, Element, Selector, Simple};

/// Au plus 4 empreintes par sélecteur (comme Stylo) : assez pour rejeter
/// presque tout, sans alourdir l'index.
pub const MAX_HASHES: usize = 4;

const TAG: u32 = 0x9e37_79b9;
const ID: u32 = 0x85eb_ca6b;
const CLASS: u32 = 0xc2b2_ae35;

/// FNV-1a sur les octets, avec une graine par genre (balise, id, classe) pour
/// que `.a` et `#a` n'aient pas la même empreinte.
fn hash(seed: u32, text: &str, lowercase: bool) -> u32 {
    let mut h = 0x811c_9dc5 ^ seed;
    for &b in text.as_bytes() {
        let b = if lowercase { b.to_ascii_lowercase() } else { b };
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    // Mélange final : les deux indices utilisent les bits bas et les bits
    // suivants, qui doivent être bien répartis.
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^ (h >> 15)
}

const SIZE: usize = 4096;

/// Un filtre de Bloom à compteurs (on retire un ancêtre en quittant son
/// sous-arbre), 2 cases par empreinte.
pub struct AncestorFilter {
    counters: Box<[u16; SIZE]>,
}

impl Default for AncestorFilter {
    fn default() -> Self {
        AncestorFilter {
            counters: Box::new([0; SIZE]),
        }
    }
}

fn slots(h: u32) -> [usize; 2] {
    [(h as usize) % SIZE, (h >> 12) as usize % SIZE]
}

impl AncestorFilter {
    fn insert(&mut self, h: u32) {
        for s in slots(h) {
            self.counters[s] = self.counters[s].saturating_add(1);
        }
    }

    fn remove(&mut self, h: u32) {
        for s in slots(h) {
            // Un compteur saturé reste saturé : il ne peut plus causer d'oubli.
            if self.counters[s] != u16::MAX {
                self.counters[s] -= 1;
            }
        }
    }

    /// Faux : l'empreinte n'est certainement pas celle d'un ancêtre.
    pub fn might_contain(&self, h: u32) -> bool {
        slots(h).iter().all(|&s| self.counters[s] > 0)
    }

    /// Ajoute (en entrant dans son sous-arbre) un élément ; ses empreintes sont
    /// ajoutées à la fin de `hashes`, pour les retirer ensuite avec `pop`.
    pub fn push<E: Element>(&mut self, element: &E, hashes: &mut Vec<u32>) {
        let start = hashes.len();
        element_hashes(element, hashes);
        for &h in &hashes[start..] {
            self.insert(h);
        }
    }

    /// Retire (en quittant son sous-arbre) un élément, avec les empreintes
    /// données par `push`.
    pub fn pop(&mut self, hashes: &[u32]) {
        for &h in hashes {
            self.remove(h);
        }
    }
}

/// Les empreintes d'un élément : sa balise, son id, ses classes.
pub fn element_hashes<E: Element>(element: &E, out: &mut Vec<u32>) {
    out.push(hash(TAG, element.local_name(), true));
    if let Some(id) = element.id() {
        out.push(hash(ID, id, false));
    }
    if let Some(classes) = element.attribute("class") {
        out.extend(
            classes
                .split_ascii_whitespace()
                .map(|c| hash(CLASS, c, false)),
        );
    }
}

/// Les empreintes que les ancêtres d'un élément doivent avoir pour que
/// `selector` puisse lui correspondre.
///
/// Une partie du sélecteur est un ancêtre du sujet quand le combinateur qui la
/// relie à sa droite est ` ` ou `>` : dans `.a > .b + .c`, `.a` est le parent de
/// `.b`, donc aussi de `.c` ; dans `.x + .y .z`, `.x` n'est qu'un frère de `.y`.
pub fn ancestor_hashes(selector: &Selector) -> ([u32; MAX_HASHES], u8) {
    let mut hashes = [0; MAX_HASHES];
    let mut n = 0;
    for (combinator, compound) in &selector.ancestors {
        if !matches!(combinator, Combinator::Descendant | Combinator::Child) {
            continue;
        }
        for simple in &compound.simple {
            let h = match simple {
                Simple::Id(id) => hash(ID, id, false),
                Simple::Class(c) => hash(CLASS, c, false),
                Simple::Type(t) => hash(TAG, t, true),
                _ => continue,
            };
            if n < MAX_HASHES {
                hashes[n] = h;
                n += 1;
            }
        }
    }
    (hashes, n as u8)
}
