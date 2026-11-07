# Commandes et environnement

## Disponibles dès ce kit

Depuis sa racine, sous PowerShell, Bash ou terminal macOS/Linux :

```sh
node scripts/validate-agent-kit.mjs
node --test scripts/tests/hooks.test.mjs
```

Node 22+ suffit, sans installation npm. La fixture PDF synthétique se régénère avec
`node scripts/make-fixtures.mjs` et se vérifie avec `node --test scripts/tests/fixtures.test.mjs`.
`node --test scripts/tests/readme.test.mjs` vérifie que les badges de versions du README
correspondent aux versions fixées (toolchain Rust, Node de la CI, `package.json`, crate Tauri).
Le plancher mémoire de l'application release se mesure avec `node scripts/measure-memory.mjs`
(voir `docs/PERFORMANCE.md`). Le validateur contrôle JSON, frontmatter du
sous-ensemble utilisé, références Markdown locales et configuration des hooks. Il
n'est pas un validateur officiel du schéma Claude Code ni un interpréteur YAML général.

Pour Claude Code : `claude --version`, puis dans une session `/status`, `/permissions`,
`/hooks`, `/agents`. Les tests Node ne demandent pas de connexion Claude et ne consomment
pas l'abonnement.

## Produit (P0)

Disponibles depuis P0 et exécutées par la CI : workflow `rust` (fmt, Clippy, tests, sur
les trois OS) et workflow `desktop` (contrôles du frontend sur Ubuntu, build natif release
sans installateur sur les trois OS).
npm, un `package-lock.json` dans `apps/desktop`, sans workspace npm racine ; workspace
Cargo racine avec `apps/desktop/src-tauri` comme membre. Prérequis : rustup (la version
vient de `rust-toolchain.toml`), Node 24, dépendances système Tauri (WebKitGTK sous Linux).
`cargo test --workspace` construit aussi `pdf-worker`, utilisé par les tests du shell.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked   # inclut les doctests
npm --prefix apps/desktop ci
npm --prefix apps/desktop run lint
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run test:unit
npm --prefix apps/desktop run build
npm --prefix apps/desktop run tauri -- build --no-bundle
cargo deny check   # licences, sources, interdictions, avis RustSec ; cargo install --locked cargo-deny
```

Scripts npm non interactifs : `lint`, `typecheck`, `test:unit`, `build`, `tauri` ;
`npm --prefix apps/desktop run tauri -- dev` lance l'application en développement. Documenter séparément les tests E2E réels et leur support par OS. Un navigateur
web de test ne remplace pas automatiquement la WebView Tauri.

Verrouiller une toolchain Rust exacte et les versions Node/npm utilisées en CI,
committer les lockfiles et installer les prérequis natifs Tauri pour chaque runner.
`cargo test --workspace` compile aussi le shell Tauri : une machine sans ses dépendances
peut tester le cœur avec les packages explicitement sélectionnés, sans déclarer le
workspace entier validé. Tester les features supportées dans une matrice dédiée.

## Contrôles périodiques à installer

`cargo audit` et `cargo deny check` après installation/versionnage et politique de
licences ; `npm --prefix apps/desktop audit` pour les dépendances JS. Inclure les
dépendances de build, codecs natifs et sidecars. Évaluer les alertes plutôt que lancer
un correctif automatique majeur. Une exception doit avoir motif, responsable et échéance.

Fuzzing (ADR-0002) : espace de travail séparé `fuzz/`, nightly daté dans
`fuzz/rust-toolchain.toml`, sous Linux ou macOS (libFuzzer). Depuis `fuzz/` :

```sh
rustup toolchain install                      # nightly de fuzz/rust-toolchain.toml
cargo install --locked cargo-fuzz@0.13.2
cargo fuzz run lexer -- -max_total_time=300 -rss_limit_mb=2048 -dict=dict/pdf.dict   # ou objects
```

Cibles : `lexer`, `objects`, `xref`, `document`. Un crash est écrit dans `fuzz/artifacts/<cible>/` ; le minimiser (`cargo fuzz tmin`)
puis en faire un test de régression dans la crate concernée. La CI (`fuzz.yml`) lance
5 minutes par cible chaque semaine et sur les PR touchant le parseur.

Oracle qpdf (ADR-0002, jamais livré) : avec qpdf installé, sous Linux ou macOS,
`CASTLE_QPDF="$(command -v qpdf)" cargo test -p pdf-syntax -p pdf-document --locked --test qpdf_oracle -- --nocapture`
compare, pour chaque fixture de `tests/corpus/public` (toutes valides), les entrées xref à
`qpdf --show-xref` et l'ordre des pages à `qpdf --show-pages`. Sans `CASTLE_QPDF`, le test est sauté ; le workflow `oracle` l'impose.

## CLI de diagnostic `castle-pdf` (crate `pdf-cli`)

Interface effective, relevée sur `--help` (version 0.1.0-alpha.1) :

```sh
cargo run -p pdf-cli -- inspect <fichier>   # castle-pdf inspect <fichier>
cargo run -p pdf-cli -- --help                # ou -h
cargo run -p pdf-cli -- --version
```

`inspect` affiche la structure (taille, version, `startxref`, révisions et sections xref,
objets en usage/libres, clés standard du trailer (les autres sont seulement comptées),
racine, pages avec objet, MediaBox, CropBox et rotation), jamais le contenu du document ni
le chemin du fichier. Le rapport est écrit ligne par ligne, chaque ligne bornée (notation
scientifique au-delà de 1e15). Seul un fichier régulier est lu. La structure du fichier
est affichée même si l'arbre des pages est invalide. Codes de sortie : 0 succès,
1 document illisible (erreur sur la sortie d'erreur), 2 usage incorrect, 3 écriture du
rapport impossible (pipe fermé). Limites par défaut de `ResourceLimits`.

Benchmarks, autres tests différentiels et vérifications de conformité seront ajoutés avec
leurs fixtures et outils. Ne pas inventer d'option à `castle-pdf` : relire son `--help`.
