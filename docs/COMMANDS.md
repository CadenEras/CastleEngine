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

Cibles : `lexer`, `objects`. Un crash est écrit dans `fuzz/artifacts/<cible>/` ; le minimiser (`cargo fuzz tmin`)
puis en faire un test de régression dans la crate concernée. La CI (`fuzz.yml`) lance
5 minutes par cible chaque semaine et sur les PR touchant le parseur.

Benchmarks, tests différentiels et vérifications de conformité seront ajoutés avec
leurs fixtures et outils. Ne pas copier une commande imaginaire de
`pdf-cli` : lire son `--help` et consigner l'interface effective lors de sa création.
