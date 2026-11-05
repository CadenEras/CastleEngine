# Organisation agentique et outils

## Ce qui est fourni

Le fichier `CLAUDE.md` contient le contrat court. Les règles sans `paths` (sécurité,
Git) s'appliquent globalement ; les autres ciblent les chemins concernés. Les skills
contiennent des procédures invoquées par `/nom`. Les sous-agents ont leur propre
contexte et une liste explicite d'outils. Cette séparation suit les mécanismes
[documentés par Claude Code](https://code.claude.com/docs/en/memory).

Les rôles lisent les documents utiles au début de leur tâche : ne pas supposer qu'un
sous-agent a lu les échanges ou les skills de la session principale.

## Permissions

`.claude/settings.json` est partagé ; `.claude/settings.local.json` reste personnel et
ignoré par Git. Le profil initial adopte `acceptEdits` pour permettre le travail local.
Choisir `default` localement si l'on veut aussi valider chaque modification ordinaire.
Les seules commandes préautorisées sont les quatre commandes exactes de diagnostic et
de validation listées dans les réglages, dupliquées pour Bash et PowerShell.

Après revue des scripts du produit en P0, ajouter seulement les commandes de contrôle
exactes utilisées quotidiennement. Ne pas autoriser `Bash(*)`, `PowerShell(*)`,
`node *`, `python *`, `cargo *` ou `npm *`. Un test, un `build.rs`, un plugin de bundler
ou un script npm peut exécuter du code avec les droits du compte : revue et environnement
de développement sans secrets restent nécessaires, même pour une commande autorisée.
Les scripts de validation du kit sont eux-mêmes du code de confiance à relire.
Les skills fournis n'ajoutent pas de champ `allowed-tools` : ils ne préautorisent donc
aucune commande supplémentaire pendant leur invocation.

Les lectures et écritures de secrets communs sont refusées. Les modifications des
réglages, hooks, configuration MCP et workflows demandent une validation du client.
Les règles de fichiers utilisent `Read` et `Edit` ; on ne crée pas une permission
`Write(path)`, qui ne constitue pas une règle de fichier prise en charge. Voir
[la référence des permissions](https://code.claude.com/docs/en/permissions).

Les règles shell sont des protections pratiques sur des formes de commandes,
pas une liste exhaustive de syntaxes équivalentes. Un programme peut lire un secret
ou exécuter Git indirectement ; les refus `Read` ne bloquent pas ces accès système.
Les glob patterns refusent aussi `.env.example` : préférer un exemple de configuration
sans ce nom, ou décider explicitement d'une exception locale étroite après revue.

## Hooks fournis

| Événement | Script | Effet |
| --- | --- | --- |
| `SessionStart` | `.claude/hooks/session-start.mjs` | Rappelle la phase et les contrôles disponibles, sans réseau ni lancement de tests |
| `PreToolUse` sur `Read`, `Edit`, `Write` | `.claude/hooks/pre-tool-use.mjs` | Bloque les secrets courants et les écritures directes dans `.git`, demande pour les réglages sensibles et les écritures hors projet |

Le hook de fichiers traite les chemins absolus/relatifs, `..`, séparateurs Windows,
casse de Windows et liens symboliques existants. Il renvoie uniquement `deny` ou
`ask`, jamais `allow` : il ne donne aucune autorisation supplémentaire. Une entrée
malformée renvoie le code 2. Aucun texte de commande issu d'une entrée n'est exécuté.
Le contexte du hook de session ne contient ni document ni variable secrète.

Les scripts sont invoqués avec Node et un chemin projet entre guillemets ; pas de
dépendance à Python, jq, chmod ou un shell POSIX dans le script. Cette forme classique
évite d'imposer les nouvelles options de hooks à l'installation locale 2.1.259.
Tester aussi le démarrage réel dans Claude Code sur chaque machine : les tests Node
vérifient le contrat JSON des scripts, pas l'intégration interactive du client.

Pas de hook `Stop` qui boucle jusqu'à réussite, ni de suite entière après chaque
frappe. Les contrôles du produit sont lancés à la fin d'une tranche cohérente et en CI.

## Options récentes, sans les activer implicitement

La [documentation actuelle des hooks](https://code.claude.com/docs/en/hooks) décrit
la forme exécutable `command` + `args`, utile pour éviter le shell, et
`onFailure: "block"` (à partir de 2.1.295). Après mise à jour, un responsable peut
convertir l'entrée du hook en :

```json
{
  "type": "command",
  "command": "node",
  "args": ["${CLAUDE_PROJECT_DIR}/.claude/hooks/pre-tool-use.mjs"],
  "onFailure": "block",
  "timeout": 5
}
```

Ce fragment remplace un handler dans `PreToolUse`, ce n'est pas un fichier settings
complet. Vérifier `/hooks` puis simuler une panne de Node dans un environnement jetable.
Dans le profil classique fourni, les pannes de lancement ou timeouts d'un hook ne sont
pas garantis bloquants : les permissions normales restent essentielles. Un code 2
émis par le script fonctionnant bloque, mais un script qui n'a pas démarré est différent.

## Agents et coût

`architect`, `pdf-specialist` et `reviewer` disposent seulement de `Read`, `Glob`,
`Grep` : leurs recommandations passent par l'agent principal pour toute exécution.
`rust-engineer`, `frontend-engineer` et `qa-engineer` disposent aussi d'édition et
de shell, avec les permissions normales. Tous héritent du modèle choisi (`inherit`) :
aucun identifiant de modèle fragile ni modèle plus cher n'est imposé.

Le périmètre de fichiers décrit dans un rôle est une consigne, pas une ACL.
Pour deux implémentations simultanées, utiliser des worktrees, transmettre un contrat
et intégrer séquentiellement. Pas d'agents en arrière-plan pour une tâche qui attend
une approbation ; ne pas donner un rôle de relecteur à l'auteur comme unique revue
d'une modification sensible. Les sous-agents consomment le quota de la session.

## Outils recommandés

- Base : Rust/Cargo, Node/npm, Git, recherche de fichiers, tests et profileurs locaux.
- En P0 : rustfmt, Clippy, audit RustSec et licences (`cargo audit`, `cargo deny`),
  analyse npm, tests React et tests natifs Tauri. Installer et verrouiller les versions
  adaptées ; ne pas annoncer des commandes comme disponibles avant l'installation.
- Pour le PDF : qpdf pour inspection structurelle et moteurs indépendants pour
  comparaison, dans des processus bornés et sans données privées. Aucun oracle ne
  prouve seul la conformité à la norme.
- Pour les performances : outils propres à chaque OS pour l'arbre de processus et
  les allocations, plus un harness reproductible décrit dans `PERFORMANCE.md`.
- MCP facultatif : source control en lecture seule, documentation ou navigateur de
  test si nécessaire. Aucun serveur ni jeton n'est configuré par défaut.

Pour ajouter MCP : évaluer l'éditeur, fixer une version, limiter dossiers/outils/réseau,
utiliser des identifiants hors Git, relire `.mcp.json` puis les outils exposés.
Un serveur MCP est un programme et une nouvelle surface d'accès, pas une permission
magique. Ne pas utiliser un `npx ...@latest` invisible à chaque démarrage.

## Isolation réelle

Le [sandbox shell de Claude Code](https://code.claude.com/docs/en/sandboxing) existe
sur macOS, Linux et WSL2 ; le Windows natif n'offre pas cette même isolation.
Il n'est donc pas activé ici avec une promesse multiplateforme trompeuse. Pour les
PDF hostiles et le fuzzing, utiliser des comptes dédiés, workers bornés ou une VM,
sans secrets, avec restrictions réseau et fichiers de l'OS. Les hooks, MCP et outils
de fichiers ne sont pas tous enfermés dans le sandbox shell.

Les politiques administrées et protections de branche peuvent imposer des contraintes
supplémentaires. Le kit ne les contourne pas et n'essaie pas de modifier la configuration
globale de Claude Code ou du poste.
