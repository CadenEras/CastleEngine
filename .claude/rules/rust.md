---
paths:
  - "crates/**"
  - "apps/desktop/src-tauri/**"
  - "**/Cargo.toml"
  - "Cargo.lock"
  - "rust-toolchain.toml"
---
# Rust

- Toolchain et MSRV explicites en P0 ; commit de `Cargo.lock` pour ce workspace applicatif.
- Privilégier Rust sûr, `Result` et erreurs structurées. Pas de `unwrap`, `expect`,
  indexation non vérifiée ou `panic!` sur des données documentaires non fiables.
- Vérifier additions, multiplications, conversions de taille et bornes avant allocation.
  Budgets transitifs sur flux, filtres, images, objets et profondeur de références.
- Éviter de cloner les octets d'un document ; propriétaire clair, slices ou buffers
  partagés immuables. Un `Arc` ne dispense pas de borner le cache ni les références.
- Pour les gros fichiers, abstraire une source à accès aléatoire. Un mapping mémoire
  impose de traiter les modifications concurrentes et l'invalidation du fichier.
- Annulation coopérative et budgets CPU ; pas de verrou global tenu durant le rendu.
- Justifier toute dépendance et feature activée ; codecs natifs et FFI exigent une revue.
- `unsafe` exceptionnel : commentaire `SAFETY`, contrat, tests négatifs, outils adaptés
  (Miri/sanitizers lorsque compatibles), relecteur distinct et ADR si structurel.
- Exécuter fmt, Clippy, tests et doctests selon `docs/COMMANDS.md`. Vérifier les
  combinaisons de features supportées, sans supposer que `--all-features` est valide.
