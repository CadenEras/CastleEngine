---
paths:
  - "apps/desktop/src/**"
  - "apps/desktop/*.json"
  - "apps/desktop/*.ts"
  - "apps/desktop/*.tsx"
---
# React et TypeScript

- TypeScript `strict`, pas de `any` de confort ; `unknown` validé aux frontières.
- Pas de parseur PDF dans React. La couche IPC convertit erreurs et DTO, puis
  présente un état explicite : chargement, prêt, annulation, limite atteinte, échec.
- Virtualiser pages et miniatures. Annuler le travail devenu invisible ; ignorer
  les résultats obsolètes avec document/génération/page/zoom/version d'édition.
- Rendre le texte extrait comme texte. Aucun HTML, script ou lien actif implicite
  provenant du PDF ; ouverture externe après geste utilisateur et contrôle d'URL.
- Libérer object URLs, listeners, textures, buffers et abonnements au démontage.
- Navigation clavier, focus visible, raccourcis, lecteurs d'écran et contraste.
- Tests utilisateur des interactions ; les mocks IPC ne prouvent pas le bon
  fonctionnement du binaire Tauri. Ajouter des tests natifs des commandes.
