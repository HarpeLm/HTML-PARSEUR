//! Robustesse : un fichier de police abîmé ou malveillant ne doit jamais faire
//! paniquer la lecture (seulement donner une erreur). On part de vraies polices
//! de la machine, tronquées ou dont des octets sont changés au hasard.
//!
//!   FUZZ_CAS=100000 cargo test --release -p lumen-font --test robustesse

use std::panic;

use lumen_font::{Font, FontDatabase, face_count};

/// Générateur pseudo-aléatoire xorshift64* : simple, rapide, reproductible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Lit la police et s'en sert : aucune panique permise.
fn exercise(data: &[u8]) {
    let count = face_count(data).unwrap_or(1).min(4);
    for index in 0..count {
        if let Ok(font) = Font::parse(data, index) {
            let _ = font.text_width("Hello, wörld € 😀 \u{10FFFF}", 16.0);
            let _ = font.line_metrics(13.0);
        }
    }
}

#[test]
fn polices_abimees_sans_panique() {
    let cases: u64 = std::env::var("FUZZ_CAS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3_000);
    let db = FontDatabase::system();
    // Quelques polices de formats différents (TrueType, collection .ttc).
    let mut samples: Vec<Vec<u8>> = ["Arial", "Times", "Georgia", "Menlo"]
        .iter()
        .filter_map(|family| {
            let face = db.faces().iter().find(|f| f.family == *family)?;
            std::fs::read(&face.path).ok()
        })
        .collect();
    samples.push(b"ttcf\0\x01\0\0\xff\xff\xff\xff".to_vec());
    samples.push(vec![0, 1, 0, 0, 0xff, 0xff]);
    if samples.len() == 2 {
        println!("⏭️  aucune police de la machine : seulement des données inventées");
    }
    for seed in 1..=cases {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut data = samples[rng.below(samples.len())].clone();
        match rng.below(3) {
            // Tronquée.
            0 => data.truncate(rng.below(data.len())),
            // Des octets changés (surtout au début : répertoire et en-têtes).
            _ => {
                for _ in 0..1 + rng.below(20) {
                    let limit = if rng.below(2) == 0 { 4096 } else { data.len() };
                    let i = rng.below(limit.min(data.len()));
                    if let Some(b) = data.get_mut(i) {
                        *b = rng.next() as u8;
                    }
                }
            }
        }
        let result = panic::catch_unwind(|| exercise(&data));
        assert!(result.is_ok(), "panique pour la graine {seed}");
    }
}
