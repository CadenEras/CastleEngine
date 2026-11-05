---
paths:
  - "crates/**"
  - "apps/desktop/**"
  - "benches/**"
---
# Performance et mémoire

- Lire `docs/PERFORMANCE.md`. Mesurer avant et après sur la même machine et le même corpus.
- Suivre la mémoire totale application + worker + WebView ; distinguer heap Rust,
  mémoire résidente, mémoire privée et GPU. Ne pas additionner deux fois les pages partagées.
- Aucune valeur fixe de RAM « garantie » parce que Tauri utilise la WebView système.
- Lecture paresseuse, index minimal, rendu en tuiles, file bornée et cache LRU compté
  en octets réels ; invalider par révision, zoom, rotation et espace couleur.
- Limiter la concurrence selon le budget mémoire, pas seulement le nombre de cœurs.
  Compter les buffers temporaires et IPC ainsi que les textures et caches de polices.
- Les optimisations conservent exactitude et limites de sécurité ; pas de `unsafe`
  pour un gain non mesuré. Publier médiane, dispersion et protocole reproductible.
