# Journal des versions

Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/).
Versions selon [SemVer](https://semver.org/) ; règles propres à `0.x` dans
[docs/GIT.md](docs/GIT.md). Une préversion `alpha` marque la fin de chaque phase de la
[feuille de route](docs/ROADMAP.md).

## [Non publié]

### Ajouté

- Licence : double licence `Apache-2.0 OR MIT` (`LICENSE-APACHE`, `LICENSE-MIT`,
  champs `license` des paquets Cargo et npm).

### Modifié

- Projet renommé CastlePDF (auparavant CastleEngine, homonyme d'un moteur de jeu) :
  dépôt, application, identifiant `io.github.cadeneras.castlepdf`, paquets et binaire
  `castle-pdf-desktop`.

## [0.1.0-alpha.1] — 2026-11-06

Fin de la phase P0 (fondations). **Aucune lecture de PDF** : cette préversion fixe
l'architecture, le worker, les budgets et la CI. Aucun binaire n'est distribué.

### Ajouté

- Workspace Cargo (édition 2024, Rust 1.99.1 fixé), lints stricts : `unsafe` interdit,
  `unwrap`/`expect`/`panic`/indexation refusés hors tests (#1).
- `pdf-syntax` : `ResourceLimits`, budget cumulatif et `CancellationToken` (#1).
- `pdf-worker` : trames préfixées par une longueur bornée avant allocation, protocole
  versionné (v1), job synthétique annulable, binaire et client avec file d'écriture
  bornée et arrêt forcé (#1, #3).
- Shell desktop Tauri 2 / React 19 / TypeScript 6 : lance le worker, commandes IPC
  validées côté Rust, permissions minimales, CSP stricte (#4).
- Fixture PDF synthétique déterministe et son manifeste (#5).
- Mesure du plancher mémoire au repos (`scripts/measure-memory.mjs`) : référence
  Windows, relevés indicatifs CI sur trois OS et WSL2 (#5, #7, #9).
- CI GitHub Actions sur Ubuntu, Windows et macOS : kit, Rust, frontend, build natif
  release ; workflow manuel de mesure mémoire (#2, #4, #7, #8).
- Protection de `main` et des tags `v*` documentée (#6).
- ADR-0001 accepté ; README du projet avec badges de versions vérifiés (#10).

### Limites connues

- Aucun PDF n'est lu ni rendu ; la lecture structurée arrive en P1.
- Pas d'installateur, de signature de binaires ni de mise à jour automatique.
- Isolation OS du worker (sandbox) non implémentée.
- Licence du projet non décidée.

[Non publié]: https://github.com/CadenEras/CastlePDF/compare/v0.1.0-alpha.1...HEAD
[0.1.0-alpha.1]: https://github.com/CadenEras/CastlePDF/releases/tag/v0.1.0-alpha.1
