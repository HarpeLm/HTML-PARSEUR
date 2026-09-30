//! Test différentiel contre Chromium : le texte dans la mise en page.
//!
//! Pages de tests/oracle/texte_cases.json : polices, hauteurs de ligne, tailles
//! mêlées, éléments en ligne imbriqués, passage à la ligne, espaces. Capture
//! Chromium : tests/oracle/texte_chromium.json. Les polices sont celles de la
//! machine (les mesures ont été prises sur macOS).
//!
//!   cargo test --release -p lumen-layout --test oracle_texte -- --nocapture

mod commun;

#[test]
fn meme_texte_que_chromium() {
    let (_, failures) = commun::compare("texte_chromium.json");
    assert!(
        failures.is_empty(),
        "{} différences avec Chromium",
        failures.len()
    );
}
