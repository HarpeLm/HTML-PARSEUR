// Capture le style calculé de Chromium pour chaque élément de pages de test.
//
// Dans la console d'un Chrome (sur une page servie en http) :
//   await captureLumenCascade(<contenu de cascade_cases.json>)
// puis enregistrer le résultat dans cascade_chromium.json.
//
// Chaque page est chargée dans une iframe de 800 × 600 px ; pour chaque élément,
// dans l'ordre du document, on relit getComputedStyle() pour PROPS.

const LUMEN_PROPS = [
  'display', 'position', 'float', 'visibility', 'box-sizing', 'opacity', 'z-index',
  'font-size', 'font-weight', 'line-height',
  'margin-top', 'margin-left', 'padding-top', 'padding-left',
];

// L'environnement d'une fenêtre, pour évaluer les media queries comme elle.
function lumenEnvironment(win) {
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
    features[name] = values.filter(v => win.matchMedia(`(${name}: ${v})`).matches).pop() ?? null;
  }
  return {
    width: win.innerWidth,
    height: win.innerHeight,
    device_width: win.screen.width,
    device_height: win.screen.height,
    resolution: win.devicePixelRatio,
    color: [...Array(17).keys()].filter(n => win.matchMedia(`(color: ${n})`).matches)[0] ?? null,
    features,
  };
}

async function lumenStylesOf(frame) {
  const doc = frame.contentDocument;
  const win = frame.contentWindow;
  return Array.from(doc.getElementsByTagName('*'), el => {
    const s = win.getComputedStyle(el);
    return [el.localName, ...LUMEN_PROPS.map(p => s.getPropertyValue(p))];
  });
}

async function captureLumenCascade(cases) {
  const frame = document.createElement('iframe');
  frame.style.cssText = 'width: 800px; height: 600px; border: 0';
  document.body.append(frame);
  const results = [];
  let environment = null;
  for (const html of cases) {
    await new Promise(resolve => { frame.onload = resolve; frame.srcdoc = html; });
    environment ??= lumenEnvironment(frame.contentWindow);
    results.push({ html, elements: await lumenStylesOf(frame) });
  }
  frame.remove();
  return {
    browser: navigator.userAgent,
    captured: new Date().toISOString().slice(0, 10),
    environment,
    props: LUMEN_PROPS,
    results,
  };
}
