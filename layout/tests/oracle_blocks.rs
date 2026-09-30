//! Test différentiel contre Chromium : la mise en page des blocs.
//!
//! Pages de tests/oracle/blocks_cases.json, sans texte : largeurs, hauteurs,
//! marges (et leur fusion), bordures, retraits. Capture Chromium :
//! tests/oracle/blocks_chromium.json.
//!
//!   cargo test -p lumen-layout --test oracle_blocks -- --nocapture

mod commun;

#[test]
fn memes_boites_que_chromium() {
    let (_, failures) = commun::compare("blocks_chromium.json");
    assert!(
        failures.is_empty(),
        "{} différences avec Chromium",
        failures.len()
    );
}
