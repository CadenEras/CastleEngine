# Vérification de la livraison

Vérification locale du kit : 3 novembre 2026, Windows, Node.js `v24.16.0`,
Claude Code observé `2.1.259`. Les commandes et résultats définitifs sont réexécutés
avant livraison.

Le diagnostic local `claude doctor` se termine avec le code 0 et n'indique pas de
problème d'installation. Cela ne remplace pas le test interactif des hooks et permissions.

## Contrôles exécutables

```sh
node scripts/validate-agent-kit.mjs
node --test scripts/tests/hooks.test.mjs
```

Le validateur vérifie les fichiers requis, JSON des réglages, forme des permissions,
enregistrement des hooks, champs de frontmatter du sous-ensemble utilisé, six rôles,
quatre skills et liens Markdown locaux. Il ne valide pas le YAML général, le schéma
officiel distant ni toutes les permissions effectives d'une installation Claude Code.

La suite de hooks comporte 10 tests : chemins ordinaires, secrets, séparateurs Windows,
traversée, métadonnées Git, politiques et scripts de contrôle, écritures hors projet,
liens symboliques, outils non concernés, JSON malformé et contexte de démarrage.
Plusieurs cas sont regroupés dans un même test. Résultat local : **9 réussis, aucun
échec, 1 ignoré**. Le test de liens symboliques/jonctions est ignoré car cet environnement
refuse leur création (`EPERM`) ; la protection correspondante n'est donc pas validée
sur ce poste. La suite la teste lorsque l'OS autorise ces fixtures.

Une relecture indépendante des permissions, hooks et procédures n'a identifié aucun
défaut bloquant ou majeur. Les scénarios « préparer une release alors que seul le kit
existe » et « valider un PDF privé sans moteur ni oracle disponible » ont été examinés.
Le second précise désormais explicitement qu'aucun diagnostic structurel ne peut être
annoncé sans outil pertinent. Cette revue documentaire n'est pas une exécution du futur
moteur ni un test de bout en bout dans Claude Code.

## Contrôles restant à faire dans le futur dépôt

- Ouvrir une session Claude Code, confirmer `/status`, `/permissions`, `/hooks`, `/agents`,
  puis tester les décisions de hook avec des données synthétiques, sans lire de secrets.
- Exécuter le workflow sur les runners Windows/macOS/Linux. Ici, aucun résultat distant
  ou multiplateforme n'est revendiqué.
- Ajouter et tester le produit en P0 : workspace Rust, frontend, IPC, worker, packaging,
  audits, corpus et mesures. Les commandes cibles documentées ne sont pas exécutées ici.
- Tester les pannes de hook et la migration éventuelle vers `onFailure: "block"` dans
  une version compatible. Le profil fourni ne garantit pas un blocage en cas de panne
  du processus de hook lui-même.

Aucun dépôt distant, push, tag, serveur MCP, abonnement ou réglage global n'est créé
ou modifié par la préparation de ce kit. La présence d'un workflow ne configure pas
automatiquement les protections de branche de l'hébergeur.
