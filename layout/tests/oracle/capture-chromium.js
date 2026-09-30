// Capture la position et la taille des boîtes de chaque élément dans Chromium.
//
// Dans la console d'un Chrome (sur une page servie en http) :
//   await captureLumenLayout(<contenu de blocks_cases.json>)
// puis enregistrer le résultat dans blocks_chromium.json.
//
// Chaque page est chargée dans une iframe de 800 × 600 px ; pour chaque élément,
// dans l'ordre du document : getBoundingClientRect() (la boîte de bordure).
// Un élément sans boîte (display: none...) donne un rectangle nul.

async function captureLumenLayout(cases) {
  const frame = document.createElement('iframe');
  frame.style.cssText = 'width: 800px; height: 600px; border: 0';
  document.body.append(frame);
  const results = [];
  let environment = null;
  for (const html of cases) {
    await new Promise(resolve => { frame.onload = resolve; frame.srcdoc = html; });
    const win = frame.contentWindow;
    environment ??= { width: win.innerWidth, height: win.innerHeight, resolution: win.devicePixelRatio };
    const elements = Array.from(frame.contentDocument.getElementsByTagName('*'), el => {
      const r = el.getBoundingClientRect();
      return [el.localName, r.x, r.y, r.width, r.height];
    });
    results.push({ html, elements });
  }
  frame.remove();
  return {
    browser: navigator.userAgent,
    captured: new Date().toISOString().slice(0, 10),
    environment,
    results,
  };
}
