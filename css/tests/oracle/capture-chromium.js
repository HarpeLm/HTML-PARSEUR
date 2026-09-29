// Capture ce que Chromium comprend de chaque déclaration de cases.json.
//
// À exécuter dans la console d'un Chrome (ou du navigateur intégré), sur
// n'importe quelle page : coller ce fichier, puis appeler
//   captureLumenOracle(<contenu de cases.json>)
// et enregistrer le résultat dans chromium.json.
//
// Pour chaque [propriété, valeur], on la donne à un élément via
// style.setProperty(), puis on relit les propriétés longues (margin-top...)
// avec leur valeur "spécifiée" sérialisée par le navigateur (CSSOM).

function captureLumenOracle(cases) {
  const el = document.createElement('div');
  const results = cases.map(([property, value]) => {
    el.style.cssText = '';
    el.style.setProperty(property, value);
    const longhands = {};
    for (const name of Array.from(el.style)) {
      longhands[name] = el.style.getPropertyValue(name);
    }
    return { property, value, valid: el.style.length > 0, longhands };
  });
  return {
    browser: navigator.userAgent,
    captured: new Date().toISOString().slice(0, 10),
    results,
  };
}
