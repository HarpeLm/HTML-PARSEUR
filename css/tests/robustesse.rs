//! Test de robustesse : du CSS aléatoire et tordu ne doit jamais faire paniquer
//! le parser, quel que soit le point d'entrée.
//!
//!   cargo test --release -p lumen-css --test robustesse                  -> 20 000 feuilles
//!   FUZZ_CAS=2000000 cargo test --release -p lumen-css --test robustesse  -> campagne longue

use std::panic;

use lumen_css::selectors::parse_selector_list;
use lumen_css::{Item, Parser, parse_an_plus_b, preprocess};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Morceaux choisis pour toucher les cas limites du tokenizer et du parser.
const PIECES: &[&str] = &[
    "a",
    "div",
    "-x",
    "--var",
    "\\",
    "\\41 ",
    "\\0",
    "\\110000",
    "\\\n",
    "#id",
    "#1",
    ".c",
    "*",
    ">",
    "+",
    "~",
    "|",
    "||",
    "~=",
    "|=",
    "^=",
    "$=",
    "*=",
    ":",
    "::",
    ";",
    ",",
    "!",
    "!important",
    "{",
    "}",
    "[",
    "]",
    "(",
    ")",
    "@media",
    "@import",
    "@",
    "url(",
    "url(x)",
    "url( \"y\" )",
    "url(a b)",
    "\"",
    "'",
    "\"str\"",
    "'s\\\nt'",
    "\"\n",
    "/*",
    "*/",
    "/* c */",
    "<!--",
    "-->",
    "1",
    "-1.5e3",
    "+.5",
    "1e",
    "12px",
    "50%",
    "3n+1",
    "-n-2",
    "odd",
    "N",
    "U+0-7F",
    "u+4??",
    "U+",
    "\0",
    "\r\n",
    "\x0C",
    " ",
    "\t",
    "é",
    "😀",
    "\u{FFFD}",
    ":not(",
    ":is(",
    ":where(",
    ":nth-child(",
    " of ",
    "[a=b i]",
    "[a",
    "rgb(",
    "calc(1px + 2%)",
    "var(--x)",
    "\\\\",
    "0.0000001",
    "99999999999999999999",
];

fn random_css(rng: &mut Rng) -> String {
    let mut css = String::new();
    for _ in 0..rng.below(60) {
        css.push_str(PIECES[rng.below(PIECES.len())]);
        if rng.below(3) == 0 {
            css.push(' ');
        }
    }
    css
}

/// Passe l'entrée par tous les points d'entrée de la bibliothèque.
fn exercise(input: &str) {
    let css = preprocess(input);
    let _ = Parser::new(&css).parse_component_value();
    let _ = Parser::new(&css).parse_declaration();
    let _ = Parser::new(&css).parse_rule();
    let _ = Parser::new(&css).parse_rule_list();
    let _ = Parser::new(&css).parse_declaration_list();
    let _ = Parser::new(&css).parse_block_contents();
    let values = Parser::new(&css).parse_component_value_list();
    let _ = parse_an_plus_b(&values);
    let _ = parse_selector_list(&values);
    // Les préludes des règles, comme le ferait un vrai moteur de style.
    for item in Parser::new(&css).parse_stylesheet() {
        if let Item::QualifiedRule(rule) = item
            && let Some(list) = parse_selector_list(&rule.prelude)
        {
            for selector in &list.0 {
                let _ = selector.specificity();
            }
        }
    }
}

#[test]
fn css_aleatoire_sans_panique() {
    let cases: u64 = std::env::var("FUZZ_CAS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000);
    for seed in 1..=cases {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let css = random_css(&mut rng);
        let result = panic::catch_unwind(|| exercise(&css));
        assert!(result.is_ok(), "panique pour la graine {seed} : {css:?}");
    }
}
