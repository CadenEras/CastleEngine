# Kit agentique — moteur PDF Rust + Tauri / React / TypeScript

Configuration éditable préparée le **3 novembre 2026**. Elle accompagne un moteur maison
et une application desktop Windows/macOS/Linux. Aucun moteur PDF n'est encore implémenté.
« Udags Git » est interprété comme **usages Git** ; les **tags** sont également couverts.

## Installation

1. Copier **le contenu** de ce dossier à la racine du futur dépôt, y compris `.claude`
   et `.github`. Dans un dépôt existant, fusionner les fichiers de configuration après
   lecture du diff, sans écraser ses conventions ou ses réglages personnels.
2. Disposer de Node.js 22 ou supérieur et de Claude Code. Aucun paquet npm n'est
   nécessaire pour les hooks et la validation du kit.
3. Depuis la racine du dépôt, vérifier les fichiers :

   ```sh
   node scripts/validate-agent-kit.mjs
   node --test scripts/tests/hooks.test.mjs
   claude --version
   ```

4. Lire les hooks puis lancer `claude`. Accepter la confiance du dépôt seulement
   après lecture de sa configuration. Dans la session, consulter `/status`,
   `/permissions`, `/hooks` et `/agents` ; vérifier que les fichiers projet sont chargés.
   Si `.claude/agents` vient d'être créé dans une session déjà ouverte, redémarrer.
5. Commencer par P0 dans [la feuille de route](docs/ROADMAP.md). Installer Rust,
   les dépendances système Tauri et le gestionnaire JS pendant cette phase.

Le mode partagé `acceptEdits` accepte les modifications ordinaires. Les commandes
shell non autorisées restent contrôlées ; aucune autorisation générale de shell,
de réseau, de serveur MCP ou de publication n'est fournie.

## Où lire quoi

| Fichier | Utilité |
| --- | --- |
| [CLAUDE.md](CLAUDE.md) | Mission, invariants et manière de travailler |
| [Configuration de l'agent](docs/AGENT-SETUP.md) | Permissions, hooks, skills, agents, outils, limites |
| [Architecture](docs/ARCHITECTURE.md) | Frontières Rust/UI/worker et contrats |
| [Commandes](docs/COMMANDS.md) | Commandes disponibles et commandes à créer en P0 |
| [Git](docs/GIT.md) | Branches, commits, PR, tags et releases |
| [Qualité](docs/QUALITY.md) | Tests et contrôles par niveau |
| [Sécurité](docs/SECURITY.md) | Menaces PDF, Tauri, sauvegarde et dépendances |
| [Performance](docs/PERFORMANCE.md) | Mesures, mémoire, cache et objectifs initiaux |
| [Phasage](docs/ROADMAP.md) | Jalons, livrables et critères de sortie |
| [Sources vérifiées](docs/SOURCES.md) | Documentation officielle et compatibilité |

## Utilisation dans Claude Code

```text
/implement-feature P0 : initialiser le workspace selon docs/ROADMAP.md
/review-diff
/validate-pdf tests/corpus/public/minimal.pdf
/prepare-release 0.1.0
```

Les deux premières procédures travaillent sur le dépôt. La troisième attend un moteur
et un fichier de test autorisé ; la dernière prépare un dossier de release sans publier.
Les exemples ne supposent pas qu'un fichier PDF ou le workspace existe déjà.

Pour une tâche complexe : demander à `architect` une analyse ; confier une tranche à
`rust-engineer` ou `frontend-engineer`, puis une relecture à `reviewer` et les cas PDF
à `pdf-specialist` ou `qa-engineer`. Éviter d'appeler les six rôles pour une petite tâche.

## Validation et portée

Le workflow `.github/workflows/agent-config.yml` vérifie le kit sur trois OS.
Les vérifications Rust, UI et packaging devront être ajoutées en P0 : elles ne sont
pas simulées ici par des étapes vertes qui ignoreraient un workspace absent.
La validation locale et ses limites sont consignées dans `docs/VALIDATION.md`.
