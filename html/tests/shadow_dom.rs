//! Shadow DOM déclaratif (`<template shadowrootmode>`), d'après la spec HTML
//! (§13.2.6.4.4, balise de début « template ») et DOM (« attach a shadow root »).
//! Les suites html5lib et WPT (format .dat) ne le couvrent pas : ces cas sont
//! écrits à la main. La page MDN de html/benches/pages/ sert de vérification
//! réelle contre Chromium (style/tests/oracle_pages.rs).

use html_parseur::dom::{Namespace, NodeData, NodeId, ShadowRootMode};
use html_parseur::{ParseOptions, parse_document_with, parse_fragment};

fn parse(html: &str) -> html_parseur::dom::Document {
    parse_document_with(
        html,
        ParseOptions {
            declarative_shadow_roots: true,
            ..ParseOptions::default()
        },
    )
}

/// L'arbre de `<body>`, au format des tests html5lib.
fn body(html: &str) -> String {
    let doc = parse(html);
    let body = doc
        .descendants(NodeId::DOCUMENT)
        .find(|&n| {
            doc.element(n)
                .is_some_and(|e| doc.atoms.name(e.name) == "body")
        })
        .unwrap();
    doc.to_test_string_from(body)
}

#[test]
fn racine_ouverte() {
    assert_eq!(
        body(
            "<div><template shadowrootmode=open><p>fantôme</p></template><span>clair</span></div>"
        ),
        "\
| <div>
|   #shadow-root (open)
|     <p>
|       \"fantôme\"
|   <span>
|     \"clair\""
    );
}

#[test]
fn racine_fermee_et_casse() {
    assert_eq!(
        body("<section><template shadowrootmode=CLOSED>x</template></section>"),
        "\
| <section>
|   #shadow-root (closed)
|     \"x\""
    );
}

#[test]
fn mode_invalide_template_ordinaire() {
    assert_eq!(
        body("<div><template shadowrootmode=bogus>x</template></div>"),
        "\
| <div>
|   <template>
|     shadowrootmode=\"bogus\"
|     content
|       \"x\""
    );
}

#[test]
fn hote_impossible() {
    // <ul> ne peut pas avoir de racine fantôme : le template reste un élément.
    assert_eq!(
        body("<ul><template shadowrootmode=open>x</template></ul>"),
        "\
| <ul>
|   <template>
|     shadowrootmode=\"open\"
|     content
|       \"x\""
    );
}

#[test]
fn element_personnalise() {
    assert_eq!(
        body(
            "<mdn-dropdown><template shadowrootmode=open><slot></slot></template>a</mdn-dropdown>"
        ),
        "\
| <mdn-dropdown>
|   #shadow-root (open)
|     <slot>
|   \"a\""
    );
    // Pas un nom d'élément personnalisé valide : pas de tiret, ou nom réservé.
    for name in ["monelement", "font-face"] {
        let tree = body(&format!(
            "<{name}><template shadowrootmode=open>x</template></{name}>"
        ));
        assert!(tree.contains("<template>"), "{name} :\n{tree}");
    }
}

#[test]
fn une_seule_racine_par_hote() {
    assert_eq!(
        body(
            "<div><template shadowrootmode=open>1</template><template shadowrootmode=open>2</template></div>"
        ),
        "\
| <div>
|   #shadow-root (open)
|     \"1\"
|   <template>
|     shadowrootmode=\"open\"
|     content
|       \"2\""
    );
}

#[test]
fn racines_imbriquees() {
    assert_eq!(
        body(
            "<div><template shadowrootmode=open><span><template shadowrootmode=closed><b>b</b></template></span></template></div>"
        ),
        "\
| <div>
|   #shadow-root (open)
|     <span>
|       #shadow-root (closed)
|         <b>
|           \"b\""
    );
}

#[test]
fn body_et_head() {
    // <body> peut être hôte ; <head> non.
    let doc = parse(
        "<head><template shadowrootmode=open>h</template></head><body><template shadowrootmode=open>b</template>",
    );
    let tree = doc.to_test_string();
    assert!(tree.contains("|   <head>\n|     <template>"), "{tree}");
    assert!(
        tree.contains("|   <body>\n|     #shadow-root (open)"),
        "{tree}"
    );
}

#[test]
fn attributs_de_la_racine() {
    let doc = parse(
        "<div><template shadowrootmode=open shadowrootclonable shadowrootdelegatesfocus></template></div>",
    );
    let div = doc
        .descendants(NodeId::DOCUMENT)
        .find(|&n| {
            doc.element(n)
                .is_some_and(|e| doc.atoms.name(e.name) == "div")
        })
        .unwrap();
    let root = doc.element(div).unwrap().shadow_root.unwrap();
    let NodeData::ShadowRoot(info) = &doc.node(root).data else {
        panic!("pas une racine fantôme");
    };
    assert_eq!(info.host, div);
    assert_eq!(info.mode, ShadowRootMode::Open);
    assert!(info.clonable && info.delegates_focus && !info.serializable);
    // La racine n'est l'enfant de personne : un parcours de l'arbre ne la voit pas.
    assert!(doc.node(root).parent.is_none());
    assert!(!doc.descendants(NodeId::DOCUMENT).any(|n| n == root));
}

#[test]
fn desactive_par_defaut_et_dans_les_fragments() {
    let html = "<div><template shadowrootmode=open>x</template></div>";
    let tree = parse_document_with(html, ParseOptions::default()).to_test_string();
    assert!(
        tree.contains("<template>") && !tree.contains("#shadow-root"),
        "{tree}"
    );

    let options = ParseOptions {
        declarative_shadow_roots: true,
        ..ParseOptions::default()
    };
    let (doc, root) = parse_fragment(html, Namespace::Html, "body", options);
    let tree = doc.to_test_string_from(root);
    assert!(
        tree.contains("<template>") && !tree.contains("#shadow-root"),
        "{tree}"
    );
}
