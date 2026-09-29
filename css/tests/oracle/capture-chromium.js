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

// ───────────── @media ─────────────
//
// captureLumenMedia(<contenu de media_cases.json>) -> media_chromium.json
//
// Pour chaque requête : sa sérialisation (matchMedia().media) et son résultat
// (matches) dans la fenêtre courante. L'environnement (taille de la fenêtre,
// densité de pixels, écran...) est enregistré avec, pour que nos tests
// évaluent les requêtes dans le même environnement.

function captureLumenMedia(cases) {
  // Pour les caractéristiques "discrètes", la valeur de cette fenêtre.
  const discrete = {
    'hover': ['none', 'hover'],
    'any-hover': ['none', 'hover'],
    'pointer': ['none', 'coarse', 'fine'],
    'any-pointer': ['none', 'coarse', 'fine'],
    'prefers-color-scheme': ['light', 'dark'],
    'prefers-reduced-motion': ['no-preference', 'reduce'],
    'prefers-reduced-transparency': ['no-preference', 'reduce'],
    'prefers-contrast': ['no-preference', 'less', 'more', 'custom'],
    'forced-colors': ['none', 'active'],
    'inverted-colors': ['none', 'inverted'],
    'scripting': ['none', 'initial-only', 'enabled'],
    'update': ['none', 'slow', 'fast'],
    'overflow-block': ['none', 'scroll', 'paged'],
    'overflow-inline': ['none', 'scroll'],
    'color-gamut': ['srgb', 'p3', 'rec2020'],
    'dynamic-range': ['standard', 'high'],
    'display-mode': ['browser', 'fullscreen', 'standalone', 'minimal-ui', 'picture-in-picture', 'window-controls-overlay'],
  };
  const features = {};
  for (const [name, values] of Object.entries(discrete)) {
    // La plus "forte" valeur qui correspond (color-gamut: p3 implique srgb).
    features[name] = values.filter(v => matchMedia(`(${name}: ${v})`).matches).pop() ?? null;
  }
  const bits = [...Array(17).keys()].filter(n => matchMedia(`(color: ${n})`).matches)[0] ?? null;
  const environment = {
    width: innerWidth,
    height: innerHeight,
    device_width: screen.width,
    device_height: screen.height,
    resolution: devicePixelRatio,
    color: bits,
    features,
  };
  const results = cases.map(query => {
    const m = matchMedia(query);
    return { query, media: m.media, matches: m.matches };
  });
  return {
    browser: navigator.userAgent,
    captured: new Date().toISOString().slice(0, 10),
    environment,
    results,
  };
}

// ───────────── Propriétés personnalisées et var() ─────────────
//
// captureLumenVariables(<contenu de variables_cases.json>) -> variables_chromium.json
//
// Chaque cas donne le style d'un parent et d'un enfant ; on relit les valeurs
// calculées (getComputedStyle) de l'enfant.

function captureLumenVariables(cases) {
  const parent = document.createElement('div');
  const child = document.createElement('div');
  parent.append(child);
  document.body.append(parent);
  const results = cases.map(c => {
    parent.style.cssText = c.parent;
    child.style.cssText = c.style;
    const style = getComputedStyle(child);
    const computed = {};
    for (const name of c.read) computed[name] = style.getPropertyValue(name);
    return { ...c, computed };
  });
  parent.remove();
  return {
    browser: navigator.userAgent,
    captured: new Date().toISOString().slice(0, 10),
    results,
  };
}
