// Temps de recalcul complet du style dans Chromium (Blink), sur les 5 vraies
// pages, pour comparaison avec lumen-style.
//
// Servir html/benches/pages/ dans un dossier pages/ avec une politique de
// sécurité qui bloque tout ce qui est externe (voir style/tests/oracle/), ouvrir
// la page d'accueil du serveur dans Chrome, coller ce fichier dans la console :
//   await mesurerRecalculStyle()
//
// Méthode : dans une iframe de 1024 × 768, on ajoute une feuille `* { --x: n }`,
// ce qui oblige Blink à recalculer le style de TOUS les éléments (règles
// retestées, cascade refaite), puis on force le calcul avec getComputedStyle()
// sur la racine. Médiane de 30 mesures, après 5 d'échauffement.
//
// Limites : la mesure comprend la mise à jour de l'arbre de rendu (pas la mise
// en page), la prise en compte de la nouvelle feuille, et passe par JavaScript
// (précision de performance.now() : 0,1 ms).

async function mesurerRecalculStyle(dossier = '/pages') {
  const pages = ['wikipedia-fr-rust', 'wikipedia-en-html', 'whatwg-parsing', 'rust-doc-vec', 'mdn-fr-table'];
  const median = a => { const s = [...a].sort((x, y) => x - y); return s[Math.floor(s.length / 2)]; };
  const frame = document.createElement('iframe');
  frame.style.cssText = 'width: 1024px; height: 768px; border: 0';
  document.body.append(frame);
  const resultats = {};
  for (const p of pages) {
    await new Promise(r => { frame.onload = r; frame.src = `${dossier}/${p}.html`; });
    const d = frame.contentDocument, w = frame.contentWindow;
    w.getComputedStyle(d.documentElement).color;
    const temps = [];
    for (let i = 0; i < 35; i++) {
      const style = d.createElement('style');
      style.textContent = `* { --lumen-bench: ${i} }`;
      d.head.append(style);
      const t0 = performance.now();
      w.getComputedStyle(d.documentElement).getPropertyValue('--lumen-bench');
      const t = performance.now() - t0;
      style.remove();
      w.getComputedStyle(d.documentElement).color;
      if (i >= 5) temps.push(t);
    }
    resultats[p] = { elements: d.getElementsByTagName('*').length, ms: median(temps) };
  }
  frame.remove();
  return { navigateur: navigator.userAgent, resultats };
}
