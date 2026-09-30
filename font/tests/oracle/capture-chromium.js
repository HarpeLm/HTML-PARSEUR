// Capture les mesures de texte de Chromium (canvas 2D, measureText).
//
// Dans la console d'un Chrome :
//   captureLumenMesures(<contenu de mesures_cases.json>)
// puis enregistrer le résultat dans mesures_chromium.json.
//
// Pour chaque [famille, taille, texte] : la largeur, et les hauteurs au-dessus
// et au-dessous de la ligne de base que Chromium donne à la police
// (fontBoundingBoxAscent / Descent). Sans crénage ni ligatures (la mise en forme
// avancée du texte viendra plus tard).

function captureLumenMesures(cases) {
  const ctx = document.createElement('canvas').getContext('2d');
  ctx.fontKerning = 'none';
  ctx.textRendering = 'optimizeSpeed';
  const results = cases.map(([family, size, text]) => {
    const quoted = /^[a-z-]+$/.test(family) ? family : `"${family}"`;
    ctx.font = `${size}px ${quoted}`;
    const m = ctx.measureText(text);
    return { family, size, text, width: m.width, ascent: m.fontBoundingBoxAscent, descent: m.fontBoundingBoxDescent };
  });
  return {
    browser: navigator.userAgent,
    captured: new Date().toISOString().slice(0, 10),
    results,
  };
}
