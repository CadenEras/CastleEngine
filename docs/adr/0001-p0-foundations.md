# ADR-0001 — Fondations P0 : toolchains, workspace, worker et budgets

Statut : accepté le 2026-11-06
Date : 2026-11-05
Responsable : Mélissa Gries (maintenance du dépôt)

## Contexte et contraintes

P0 doit livrer un workspace, une UI vide, un protocole worker, des budgets, un corpus
et une CI sur trois OS (`docs/ROADMAP.md`). Versions relevées le 2026-11-05 (canal
stable Rust, `npm view`, crates.io), puis installées et compilées ensemble : localement
sous Windows 11 x64 (Rust 1.99.1, Node 24.16.0, npm 11.6.4) et en CI sur Ubuntu,
Windows et macOS. Proposée le 2026-11-05, acceptée sans modification le 2026-11-06.

## Options

- **Toolchain Rust** : canal `stable` flottant (simple, mais builds non reproductibles)
  ou version exacte dans `rust-toolchain.toml` (mise à jour explicite par PR).
- **Worker** : thread dans le processus Tauri (simple, aucune isolation, un panic ou une
  boucle bloque l'application) ou sous-processus dédié (arrêtable, isolable par l'OS,
  coût d'un protocole et de la sérialisation).
- **Protocole** : JSON ligne par ligne (lisible, taille non bornée sans effort) ou
  trames préfixées par une longueur bornée (borne vérifiée avant allocation).
- **Dépendances du cœur en P0** : aucune (std seule) ou bibliothèques dès maintenant.

## Décision

| Sujet | Décision |
| --- | --- |
| Rust | 1.99.1 exacte via `rust-toolchain.toml`, édition 2024, MSRV = 1.99 |
| Node / npm | Node 24 LTS, npm 11 ; versions exactes à fixer dans la CI |
| Tauri / React / TS / Vite | Crate `tauri` 2.12.5, CLI et `@tauri-apps/api` 2.12.4, React 19.3.1, Vite 8.3.6, Vitest 5.1.0, ESLint 10.14.0. **TypeScript 6.0.3** et non 7.0.2 : `typescript-eslint` 8.72.1 exige TypeScript < 6.1. Versions exactes dans `package-lock.json` et `Cargo.lock` |
| Identifiant d'application | `io.github.cadeneras.castleengine` (persistant : chemins de données par OS) |
| Lancement du worker | Binaire `pdf-worker` à côté de l'exécutable du shell, lancé par `std::process`, sans plugin shell Tauri. L'empaquetage en sidecar est traité avec les installateurs |
| Permissions Tauri | Commandes applicatives déclarées dans `build.rs` ; capability `default` limitée à l'écoute d'événements et aux deux commandes du worker ; CSP sans script distant ni `unsafe-inline` hors développement |
| OS minimums | Windows 10 22H2 / 11 x64 en priorité ; macOS 13+ ; Linux x86_64 avec WebKitGTK 4.1 (base Ubuntu 22.04). ARM64 hors périmètre P0 |
| Workspace | Cargo à la racine, resolver 3 ; crates créées à mesure du besoin, en commençant par `pdf-syntax` et `pdf-worker` ; `apps/desktop/src-tauri` rejoint le workspace quand le shell existe |
| Lints | `unsafe_code = "forbid"` ; Clippy `unwrap_used`, `expect_used`, `panic`, `indexing_slicing` refusés hors tests |
| Budgets | `ResourceLimits` + `Budget` cumulatif + `CancellationToken` dans `pdf-syntax`, couche la plus basse, partagée par toutes les autres |
| Worker | Sous-processus dédié. Trames `u32` little-endian + charge utile, taille maximale vérifiée avant allocation, version de protocole explicite, identifiant de requête et message d'annulation |
| Dépendances | Aucune en P0. Décompression Flate, polices et rasterisation : ADR dédiée en P1/P2 (licence, sécurité, mémoire, maintenance) |
| Profil PDF initial | PDF 1.x, xref classique, sans chiffrement ; toute capacité absente renvoie une erreur `Unsupported` explicite |
| Licence du projet | Non décidée : `publish = false`, pas de champ `license` inventé |

## Conséquences et validation

- Contrôles à exécuter dès qu'une toolchain est installée : `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo test --workspace --locked`, puis commit du `Cargo.lock` généré.
- Tests du premier jalon : limite exacte acceptée, limite + 1 refusée sans modifier le
  compteur, annulation observée en cours de tâche, trame trop grande rejetée avant
  allocation, trame tronquée signalée.
- Fixture synthétique : `tests/corpus/public/minimal.pdf`, générée et vérifiée par script.
  Plancher mémoire : référence Windows `docs/perf/2026-11-05-p0-windows.md` ;
  relevés indicatifs macOS et Linux dans `docs/PERFORMANCE.md`.
- Isolation OS du worker (Job Objects / AppContainer, sandbox macOS, seccomp/Landlock) :
  à étudier par OS ; aucune isolation n'est revendiquée avant tests dédiés.
- Retour arrière : les versions sont modifiables par PR ; le protocole est versionné
  pour permettre une évolution incompatible détectée plutôt que silencieuse.
