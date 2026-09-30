//! Le crénage, contre les largeurs mesurées dans une page par Chromium 152
//! (`getBoundingClientRect()` d'un `<span>` de 32px, sur macOS) : « AVATAR To »
//! se resserre (A et V, V et A, T et o...).
//!
//! Arial, Verdana et Times New Roman portent leur crénage dans la table `GPOS` ;
//! Times et Helvetica (versions macOS) dans l'ancienne table `kern` d'Apple.
//! Les polices absentes de la machine sont sautées.

use lumen_font::FontDatabase;

#[test]
fn memes_largeurs_que_chromium() {
    let db = FontDatabase::system();
    let cases = [
        ("Arial", 174.265625, 160.640625),
        ("Verdana", 179.859375, 170.796875),
        ("Times New Roman", 176.875, 159.703125),
        ("Times", 176.875, 159.734375),
        ("Helvetica", 174.265625, 160.71875),
    ];
    let mut checked = 0;
    for (family, without, with) in cases {
        let Some(font) = db.query(family, 400, false) else {
            println!("⏭️  {family} absente de cette machine");
            continue;
        };
        checked += 1;
        assert_eq!(
            font.text_width("AVATAR To", 32.0),
            without,
            "{family} sans crénage"
        );
        assert_eq!(
            font.kerned_width("AVATAR To", 32.0),
            with,
            "{family} avec crénage"
        );
    }
    println!("{checked} polices vérifiées");
}
