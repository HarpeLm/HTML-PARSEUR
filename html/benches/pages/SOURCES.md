# Pages réelles du benchmark : sources et licences

Ces pages servent uniquement à mesurer les performances du parser
(`benches/vraies_pages.rs`). Elles ont été téléchargées le **29 septembre 2026**
et sont conservées **sans aucune modification** (empreintes SHA-256 ci-dessous).

**Ces fichiers ne sont PAS couverts par la licence MIT / Apache-2.0 du projet** :
chacun reste sous sa propre licence, indiquée ci-dessous. Ils sont exclus du
paquet publié (`exclude` dans `Cargo.toml`).

| Fichier | Source | Licence | SHA-256 (début) |
|---|---|---|---|
| `wikipedia-fr-rust.html` | [Rust (langage)](https://fr.wikipedia.org/wiki/Rust_(langage)), Wikipédia en français | [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/deed.fr) | `f9cbe6324eadff03` |
| `wikipedia-en-html.html` | [HTML](https://en.wikipedia.org/wiki/HTML), Wikipedia in English | [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/) | `ebebfd11963bb86a` |
| `whatwg-parsing.html` | [HTML Standard, §13.2 Parsing HTML documents](https://html.spec.whatwg.org/multipage/parsing.html), WHATWG | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) | `660822fb01c70c41` |
| `rust-doc-vec.html` | [`std::vec::Vec`](https://doc.rust-lang.org/std/vec/struct.Vec.html), documentation de Rust | MIT OR Apache-2.0 | `1b60333eef69c340` |
| `mdn-fr-table.html` | [`<table>`](https://developer.mozilla.org/fr/docs/Web/HTML/Element/table), MDN Web Docs en français | [CC BY-SA 2.5](https://creativecommons.org/licenses/by-sa/2.5/) ou ultérieure | `135348c65ae40088` |

## Auteurs et attribution

- **Wikipédia / Wikipedia** : les contributeurs de chaque article ; la liste
  complète est dans l'historique de la page :
  [Rust (langage)](https://fr.wikipedia.org/w/index.php?title=Rust_(langage)&action=history),
  [HTML](https://en.wikipedia.org/w/index.php?title=HTML&action=history).
- **HTML Standard** : © WHATWG (Apple, Google, Mozilla, Microsoft). Licence
  indiquée dans la [politique de propriété intellectuelle du WHATWG](https://whatwg.org/ipr-policy),
  §7.1.1 (« Living Standards … are licensed under CC BY 4.0 »).
- **Documentation de Rust** : © The Rust Project Contributors. Licence indiquée
  dans le fichier [COPYRIGHT](https://github.com/rust-lang/rust/blob/master/COPYRIGHT)
  du projet.
- **MDN Web Docs** : © 1998–2026 les contributeurs individuels de mozilla.org.
  Licence indiquée sur la page
  [Attributions and copyright licensing](https://developer.mozilla.org/en-US/docs/MDN/Writing_guidelines/Attrib_copyright_license).

Les images, polices et scripts externes auxquels ces pages font référence ne sont
pas inclus : seul le fichier HTML de chaque page est conservé.

## Vérifier l'intégrité

```bash
shasum -a 256 benches/pages/*.html
```
