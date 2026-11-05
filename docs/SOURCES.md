# Documentation consultée et compatibilité

Vérification en ligne : **3 novembre 2026**. Les pages ci-dessous sont les sources
primaires utilisées pour les formats de configuration et les principes mentionnés.
Le contenu du kit est une proposition propre au projet, pas une configuration officielle
Anthropic ni une copie de ces documents. Les choix d'architecture, budgets, workflow Git
et jalons sont des recommandations à calibrer.

## Claude Code

| Source officielle | Point vérifié |
| --- | --- |
| [Memory et rules](https://code.claude.com/docs/en/memory) | `CLAUDE.md`, `.claude/rules/*.md`, ciblage `paths` |
| [Settings](https://code.claude.com/docs/en/settings) | Réglages projet/locaux et schéma JSON déclaré |
| [Permissions](https://code.claude.com/docs/en/permissions) | Modes, règles Bash/PowerShell, `Read`/`Edit`, portée et limites |
| [Hooks](https://code.claude.com/docs/en/hooks) | Événements, entrée stdin, JSON de décision, chemins, erreurs et formes d'exécution |
| [Skills](https://code.claude.com/docs/en/skills) | `.claude/skills/<nom>/SKILL.md`, frontmatter et invocation `/nom` |
| [Sous-agents](https://code.claude.com/docs/en/sub-agents) | Frontmatter des rôles, outils, modèle et contexte propre |
| [Sandbox shell](https://code.claude.com/docs/en/sandboxing) | Disponibilité macOS/Linux/WSL2 et absence de la même isolation sur Windows natif |

Commande locale observée : `claude --version` → `2.1.259 (Claude Code)`.
Ce n'est pas une affirmation qu'il s'agit de la dernière version disponible.
Le profil principal emploie des mécanismes établis et la forme classique des hooks.
La nouvelle option `onFailure: "block"` est documentée à partir de 2.1.295 et reste
une migration optionnelle dans `AGENT-SETUP.md`, sans modification de l'installation.

Le schéma indiqué par `$schema` apporte l'aide des éditeurs ; le validateur local ne
le télécharge pas et ne prétend pas certifier la compatibilité du client. Un schéma
public peut aussi suivre les évolutions avec retard. Le contrôle d'intégration final
se fait dans Claude Code avec les réglages et politiques réellement chargés.

## PDF et Tauri

- [PDF Association — ISO 32000-2](https://pdfa.org/resource/iso-32000-2/) : point d'accès
  à la spécification PDF. Consulter l'édition et ses errata au moment de l'implémentation ;
  ce kit ne prétend pas avoir vérifié toutes les clauses de la norme.
- [Tauri — Security](https://v2.tauri.app/security/) et
  [Capabilities](https://v2.tauri.app/security/capabilities/) : frontières UI/Rust,
  permissions et capacités. Aucun fichier de capabilities applicatif n'est inventé
  avant de connaître les commandes, fenêtres et plugins du produit.

## Git et CI

- [Conventional Commits 1.0](https://www.conventionalcommits.org/en/v1.0.0/) : syntaxe
  des messages ; les scopes et types supplémentaires sont des conventions du projet.
- [SemVer 2.0](https://semver.org/) : sens des versions ; la politique détaillée avant
  `1.0` est fixée explicitement dans `GIT.md`.
- [Commit checkout v4.2.2](https://github.com/actions/checkout/commit/11bd71901bbe5b1630ceea73d27597364c9af683) :
  identité du commit épinglé du workflow de validation. Cet épinglage vérifié n'est pas
  une affirmation qu'il s'agit de la dernière version ; le mettre à jour par PR revue.

## Entretenir ce kit

Lors d'une mise à jour Claude Code : noter la version et la date, relire les rubriques
touchées, revoir le diff des réglages puis exécuter contrôles locaux et essai interactif.
Ne pas ajouter un nouveau champ de frontmatter sur la seule base d'un exemple de blog.
Les contrôles locaux utilisent volontairement un sous-ensemble strict : les faire évoluer
si des champs officiellement pris en charge deviennent utiles au dépôt.
