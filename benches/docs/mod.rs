//! Documents HTML générés, partagés entre le benchmark et le profilage.
//! (inclus avec `#[path]`, ce n'est pas un benchmark à lui seul)

#![allow(dead_code)]

use std::fmt::Write;

const LOREM: &str = "Le navigateur lit la page caractère par caractère et construit des tokens. \
                     Chaque balise, chaque attribut et chaque morceau de texte passe par la machine à états. ";

/// Page de blog typique : un mélange de tout.
pub fn blog_page(articles: usize) -> String {
    let mut s = String::from(
        "<!DOCTYPE html>\n<html lang=\"fr\">\n<head>\n<meta charset=\"utf-8\">\n\
         <title>Mon blog &mdash; accueil</title>\n\
         <link rel=\"stylesheet\" href=\"/style.css?v=3&amp;theme=dark\">\n\
         <style>body { margin: 0 } .post > h2 { color: #333 }</style>\n</head>\n<body>\n",
    );
    for i in 0..articles {
        write!(
            s,
            "<article class=\"post post-{i}\" id=\"article-{i}\" data-author=\"auteur{a}\">\n\
             <!-- article {i} -->\n\
             <h2><a href=\"/articles/{i}?ref=home&amp;page=1\">Titre de l&apos;article n&deg;{i}</a></h2>\n\
             <p class=\"meta\">Publié le <time datetime=\"2026-09-{d:02}\">{d} septembre</time> &middot; 5&nbsp;min</p>\n\
             <p>{LOREM}{LOREM}<strong>Important</strong> : l&rsquo;entité &eacute; et &#233; et &#x20AC;.</p>\n\
             <ul><li>Premier point</li><li>Deuxième point</li><li><em>Troisième</em> point</li></ul>\n\
             <img src=\"/img/{i}.webp\" alt=\"Illustration {i}\" width=640 height=360 loading=lazy>\n\
             </article>\n",
            a = i % 7,
            d = i % 28 + 1,
        )
        .unwrap();
    }
    s.push_str("<script>\nfor (let i = 0; i < 10; i++) { if (i < 5 && i > 1) console.log(\"<b>\" + i); }\n</script>\n</body>\n</html>\n");
    s
}

/// Beaucoup de balises et d'attributs, très peu de texte.
pub fn tag_heavy_page(rows: usize) -> String {
    let mut s = String::from("<!DOCTYPE html><table class=\"data\">\n");
    for i in 0..rows {
        write!(
            s,
            "<tr id=\"r{i}\" class=\"row\"><td class=\"c1\">{i}</td><td class=\"c2\" data-v=\"{v}\">{v}</td>\
             <td><input type=\"checkbox\" name=\"sel\" value=\"{i}\" checked></td><td><br/></td></tr>\n",
            v = i * 3
        )
        .unwrap();
    }
    s.push_str("</table>");
    s
}

/// Presque uniquement du texte.
pub fn text_heavy_page(paragraphs: usize) -> String {
    let mut s = String::from("<!DOCTYPE html><body>\n");
    for _ in 0..paragraphs {
        s.push_str("<p>");
        for _ in 0..8 {
            s.push_str(LOREM);
        }
        s.push_str("</p>\n");
    }
    s
}

