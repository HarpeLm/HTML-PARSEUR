// Mesure le parsing HTML de Chromium (DOMParser) sur les 5 pages de
// benches/pages/, pour comparaison indicative avec html-parseur.
//
// Servir le dossier benches/ en local (par exemple
// `python3 -m http.server 8731 --directory html/benches`), ouvrir
// http://localhost:8731/ dans Chrome, coller ce fichier dans la console puis
// exécuter : await mesurerChromium()

async function mesurerChromium(dossier = '/pages') {
  const pages = ['wikipedia-fr-rust', 'wikipedia-en-html', 'whatwg-parsing', 'rust-doc-vec', 'mdn-fr-table'];
  const median = a => { const s = [...a].sort((x, y) => x - y); return s[Math.floor(s.length / 2)]; };
  const resultats = {};
  for (const p of pages) {
    const texte = await (await fetch(`${dossier}/${p}.html`)).text();
    const octets = new TextEncoder().encode(texte).length;
    const parser = new DOMParser();
    for (let i = 0; i < 5; i++) parser.parseFromString(texte, 'text/html'); // échauffement
    const temps = [];
    let noeuds = 0;
    for (let i = 0; i < 30; i++) {
      const t0 = performance.now();
      const doc = parser.parseFromString(texte, 'text/html');
      temps.push(performance.now() - t0);
      if (i === 0) {
        // Même décompte que html-parseur : le document et tous ses descendants.
        const w = doc.createTreeWalker(doc, NodeFilter.SHOW_ALL);
        noeuds = 1;
        while (w.nextNode()) noeuds++;
      }
    }
    const ms = median(temps);
    resultats[p] = { noeuds, ms, mo_s: (octets / 1048576) / (ms / 1000) };
  }
  return { navigateur: navigator.userAgent, resultats };
}
