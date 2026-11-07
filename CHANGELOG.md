# Journal des versions

Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/).
Versions selon [SemVer](https://semver.org/) ; règles propres à `0.x` dans
[docs/GIT.md](docs/GIT.md). Une préversion `alpha` marque la fin de chaque phase de la
[feuille de route](docs/ROADMAP.md).

## [Non publié]

### Ajouté

- `pdf-syntax` : tokenizer strict (ISO 32000-2, 7.2–7.3) avec offsets, erreurs
  `Malformed`/`Truncated` localisées et chaînes/noms décodés comptés dans le budget ;
  source d'entrée en mémoire bornée par la nouvelle limite `max_input_bytes` (512 Mio).
- `pdf-syntax` : objets PDF (ISO 32000-2, 7.3) — tableaux, dictionnaires à clés
  uniques, références `n g R`, objets indirects et streams localisés par `/Length`
  (direct ou résolu par l'appelant) sans copie des données, `endstream` vérifié
  octet par octet ; profondeur bornée (pile explicite, plafond dur de 256), chaque
  objet compté dans le budget, erreurs localisées. Première erreur de capacité :
  `Unsupported(ExternalStreamData)` pour les streams à fichier externe (`/F`).
- `pdf-syntax` : structure de fichier (ISO 32000-2, 7.5) — trait `Source` à accès
  aléatoire, `PdfFile` : en-tête `%PDF-1.0` à `2.0`, `startxref`/`%%EOF`, tables xref
  classiques à entrées de 20 octets, trailer, mises à jour incrémentales (`/Prev`,
  cycles détectés), résolution d'objet avec contrôle d'identifiant et `/Length`
  indirect sans récursion ; référence vers un objet libre ou absent = `null`.
  Refus explicites : xref streams et fichiers hybrides, chiffrement, version inconnue.
- Fixture `incremental.pdf` (deux révisions) et test différentiel contre `qpdf`
  (`--check`, `--show-xref`) dans le workflow non requis `oracle`.
- Tests de propriétés (`proptest`) et fuzzing (`cargo-fuzz`, cibles `lexer`, `objects` et `xref`, workflow
  `fuzz` borné en temps).
- ADR-0002 accepté : stratégie de lecture P1.
- CI : `cargo-deny` (contrôle requis `deny`, workflow `advisories` pour RustSec),
  Dependabot (Cargo, npm, actions), cache Rust écrit seulement depuis `main`.
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
