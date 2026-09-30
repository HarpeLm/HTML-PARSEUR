# lumen-font

Lecture des polices TrueType et OpenType (`.ttf`, `.otf`, collections `.ttc`) et
mesure du texte, sans dépendance. C'est une brique de [Lumen](../README.md), un
navigateur web écrit de zéro : la mise en page a besoin de savoir quelle place
prend un texte et quelle hauteur fait une ligne.

- **Le format sfnt** : répertoire des tables, collections ; tables `head`,
  `hhea`, `OS/2`, `maxp`, `hmtx` (largeur des glyphes), `cmap` formats 4 et 12
  (caractère -> glyphe), `name` (famille et style). Toutes les lectures vérifient
  les bornes : un fichier abîmé donne une erreur, jamais une panique.
- **Les polices installées** (`FontDatabase`) : au démarrage, seules les petites
  tables `name`, `OS/2` et `head` sont lues (872 polices en 170 ms sur macOS) ;
  une police n'est chargée qu'à sa première utilisation. Choix d'une police pour
  une famille CSS (graisse et style les plus proches), familles génériques comme
  Chromium sur Mac : `serif` -> Times, `sans-serif` -> Helvetica,
  `monospace` -> Menlo.
- **Les mesures** : largeur d'un texte (somme des avances des glyphes), métriques
  de ligne calculées comme Chromium sur Mac (celles de `hhea`, arrondies au
  pixel ; pour Times, Helvetica et Courier, 15 % ajoutés au-dessus de la ligne de
  base pour ressembler à leurs équivalents Windows).

```rust
use lumen_font::FontDatabase;

let db = FontDatabase::system();
let font = db.query("Arial", 400, false).unwrap();
assert_eq!(font.text_width("Hello", 16.0), 36.4609375); // comme Chromium
let m = font.line_metrics(16.0); // ascent 14, descent 3, line_gap 1
```

## Vérification contre Chromium

| Test | Résultat |
|---|---|
| Largeurs et métriques (`measureText` de Chromium 152) : 11 familles, 5 tailles, 9 textes (accents, ponctuation, espaces insécables...) | **495 / 495** identiques |
| Robustesse : vraies polices tronquées ou modifiées au hasard (`tests/robustesse.rs`) | **300 000 fichiers, 0 panique** |

Les polices sont celles **installées sur la machine** (Chromium utilise les
mêmes fichiers) : rien n'est téléchargé ni redistribué, le dépôt ne contient que
les mesures. Le test saute les familles absentes (les mesures ont été prises sur
macOS).

Observé en comparant : le canvas de Chromium mesure à 13,33px une police
demandée à 13,333333px (il coupe la taille à 2 décimales) ; cette taille est donc
laissée de côté ici, et la précision des tailles dans la page sera vérifiée avec
la mise en page du texte.

## Pas encore fait

- Crénage (tables `kern`, `GPOS`), ligatures, et toute la mise en forme avancée
  (écritures arabes, indiennes...) ;
- Police de secours quand un caractère manque (un emoji dans du texte en Arial) ;
- Dessin des glyphes (tables `glyf`, `CFF`) : pour le rendu.

## Licence

Au choix : [Apache 2.0](LICENSE-APACHE) ou [MIT](LICENSE-MIT).
