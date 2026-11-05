# Usages Git, versions et releases

Choix initial : trunk-based development simple. `main` intégrable, branches courtes
et PR, sans branche `develop` ni longues branches par phase. Ce choix réduit les
conflits d'un petit projet ; plusieurs versions maintenues pourront justifier plus
tard des branches `release/1.2`.

## Branches et travail quotidien

| Usage | Exemple |
| --- | --- |
| Fonction | `feat/pdf-xref-streams` |
| Correctif | `fix/writer-truncated-output` |
| Optimisation | `perf/render-tile-cache` |
| Documentation | `docs/agent-setup` |
| Entretien / dépendances | `chore/update-codecs` |
| Expérience jetable | `spike/font-cache` |
| Correctif urgent | `hotfix/limit-image-allocation` |

Avant une branche, vérifier un état propre ou identifier précisément les changements
existants. Mettre `main` à jour en fast-forward, puis créer la branche :

```sh
git status
git switch main
git pull --ff-only
git switch -c feat/pdf-xref-streams
```

Ne pas exécuter cette séquence au milieu du travail d'autrui. Un agent dans un worktree
ne doit pas changer la branche de la copie principale. Pour des travaux concurrents,
un worktree et un périmètre distinct par implémenteur ; intégrer et tester un diff à la fois.

Pour ajouter une correction : inspecter le diff, indexer les fichiers précis,
contrôler le diff indexé, puis commit si l'utilisateur l'a demandé ou autorisé.
Pas de `--no-verify` pour contourner les contrôles. Le message du commit indique
le comportement et le motif, pas seulement « fix stuff ».

## Commits

Convention [Conventional Commits 1.0](https://www.conventionalcommits.org/en/v1.0.0/) :

```text
feat(parser): support classic cross-reference tables
fix(writer): preserve source after failed replacement
perf(render): cap decoded tile cache by bytes
test(parser): cover cyclic indirect references
docs(security): document unsupported PDF actions
```

Types : `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `build`, `ci`, `chore`,
`revert`. Scopes usuels : `parser`, `model`, `render`, `writer`, `ipc`, `desktop`,
`security`, `deps`. Une rupture d'API emploie `!` et un pied de message :

```text
feat(api)!: require explicit resource limits

BREAKING CHANGE: document opening now requires a ResourceLimits value.
```

L'IA ne s'attribue pas une identité humaine. Une éventuelle attribution suit la
politique du dépôt et les exigences de l'outil, sans inventer de coauteur.

## Pull requests et protections

Une PR correspond à un résultat relisible, idéalement quelques jours de travail.
Séparer un déplacement massif de fichiers d'une modification sémantique.
Employer `.github/pull_request_template.md` et un titre Conventional Commits.
Le titre devient le commit final avec le squash merge recommandé.

Configurer côté hébergeur (les fichiers locaux ne le font pas) : interdire force push
et suppression de `main`, exiger PR, conversations résolues, contrôles requis et
protection des tags `v*`. Une personne seule peut relire son diff après une pause,
mais ne doit pas configurer une approbation indépendante impossible à obtenir.
En équipe, exiger un autre relecteur, particulièrement pour parsing, `unsafe`, IPC,
permissions et sauvegarde. La revue d'un agent ne vaut pas approbation de compte humain.

Configuration GitHub en place depuis la fin de P0 (2026-11-05) :

- Ruleset `Protect` sur la branche par défaut : suppression et force push interdits,
  PR obligatoire sans approbation imposée, conversations résolues, squash merge seul,
  branche à jour avec `main` avant fusion.
- Contrôles requis, émis par GitHub Actions : `validate (<os>)` (kit, hooks, fixtures),
  `checks (<os>)` (fmt, Clippy, tests Rust), `build (<os>)` (build natif release) pour
  `ubuntu-latest`, `windows-latest` et `macos-latest`, plus `frontend` (lint, types,
  tests et build UI).
- Ruleset `Protect release tags` sur `refs/tags/v*` : ni suppression ni déplacement.
- Dépôt : squash merge seul, titre de PR comme message, branche supprimée après fusion.
- GitGuardian (application GitHub externe) s'exécute mais n'est pas un contrôle requis.

Tout renommage de workflow ou de job doit mettre à jour la liste des contrôles requis.
Ne pas employer de filtre de chemins qui laisserait un contrôle requis absent.

## Synchronisation et annulation

Rebase possible sur sa branche privée après coordination ; merge de `main` possible
sur branche partagée. Résoudre les conflits en comprenant les deux comportements,
puis relancer les tests concernés. Ne pas choisir automatiquement « ours »/« theirs ».

Préférer `git revert` sur un commit partagé. `reset --hard`, `clean -fd` ou une
réécriture publique ne sont pas des gestes de nettoyage ordinaires. Pour une branche
privée explicitement autorisée, préférer `--force-with-lease` au force push ; vérifier
la référence distante et l'absence de travail d'autrui. Ne jamais forcer `main`.

## Versions et tags

Adopter [SemVer 2.0](https://semver.org/) pour les contrats publiés : API Rust, IPC,
formats persistants et comportement documenté. Version du produit et des crates
publiées synchronisée au début ; crates internes sans publication indépendante.
Tauri et le package frontend reprennent la même version de release.

- `0.1.0` : première version expérimentale. Avant `1.0`, règle locale explicite :
  rupture publique augmente le MINOR, correctif compatible augmente PATCH.
  Documenter les ruptures même durant `0.x`.
- Après `1.0` : rupture MAJOR, fonctionnalité compatible MINOR, correctif PATCH.
  Conventional Commits aide à calculer la version, sans remplacer la revue du contrat.
- Préversions : `0.2.0-alpha.1`, `0.2.0-beta.1`, `0.2.0-rc.1`.
- Tag annoté `v0.2.0`, attaché au commit effectivement construit et validé.
  Signer les tags si une infrastructure de clés est en place ; ne pas inventer une clé.
- Ne jamais déplacer un tag publié ; publier une nouvelle version ou retirer une
  release compromise avec un avis explicite et traçable.

## Release

1. Préparer une PR : versions Cargo/Tauri/package, lockfiles, changelog, compatibilité,
   migrations, dépendances, notes de limites et corpus pris en charge.
2. Vérifier les tests, comparaisons PDF, budgets et packaging des OS cibles.
   Produire inventaire/SBOM, hashes et notes ; analyser les alertes et exceptions.
3. Fusionner selon l'autorisation reçue et vérifier le commit final de `main`.
   Revalider ce commit si son contenu diffère de celui testé avant fusion.
4. Après autorisation de release, créer le tag annoté (ou signé) exact, puis pousser
   cette référence explicitement. Pas de `git push --tags` qui publie d'autres tags.
5. Construire depuis le tag immuable, signer les binaires par OS, vérifier les signatures
   de mise à jour si activée, publier puis tester téléchargement/installation.

La signature d'un tag Git n'est pas la signature Windows/macOS du logiciel et n'est
pas une signature numérique d'un document PDF. Les clés restent dans le gestionnaire
de secrets ; une PR non fiable n'a jamais accès aux clés ou à un token de publication.
Le skill `/prepare-release` prépare et vérifie ; son invocation n'autorise pas la publication.
