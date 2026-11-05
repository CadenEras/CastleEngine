# Git et collaboration

- Lire `docs/GIT.md`. `main` intégrable, branches courtes, PR centrées sur un résultat.
- Vérifier l'état et les modifications existantes avant de travailler. Ne pas effacer,
  remiser ou inclure le travail d'autrui sans coordination.
- Noms : `feat/pdf-xref-streams`, `fix/render-clipping`, `docs/agent-setup`.
- Commits Conventional Commits : `feat(parser): read classic xref tables`.
  Commits petits et cohérents ; indexer les chemins voulus, jamais `git add .` par réflexe.
- Commit local seulement si demandé ou autorisé ; push, PR externe, merge, tag distant
  et publication selon les instructions reçues, sans réinterroger une autorisation acquise.
- Pas de réécriture de branche partagée ni de force push de `main`. Préférer `revert`.
  `--force-with-lease` seulement pour sa branche privée et si autorisé.
- Versions SemVer, tags annotés `v0.1.0`, préversions `v0.2.0-alpha.1` ; ne jamais
  déplacer un tag publié. Suivre la procédure de release de `docs/GIT.md`.
- Une revue d'agent apporte des constats ; elle ne remplace pas une approbation humaine
  exigée par les protections du dépôt.
