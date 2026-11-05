---
paths:
  - "crates/**"
  - "apps/desktop/**"
  - "Cargo.toml"
---
# Architecture

- Lire `docs/ARCHITECTURE.md`. Les dépendances vont de l'interface vers le cœur.
- Conserver les octets, références indirectes, objets et erreurs dans `pdf-syntax` ;
  le modèle de pages dans `pdf-document` ; l'interprétation graphique dans `pdf-render` ;
  la sérialisation dans `pdf-write`. Éviter une crate par microtype.
- Le shell Tauri ne fait que valider les commandes, gérer les documents et orchestrer
  le worker. La logique PDF ne vit ni dans les composants React ni dans l'IPC.
- Utiliser des identifiants opaques de document et des DTO versionnés et typés.
  Éviter d'envoyer le PDF entier ou chaque tuile en base64 à travers l'IPC.
- Parsing et rendu coûteux dans le worker isolable, jamais dans le thread UI.
  Une tâche asynchrone n'est pas en elle-même une isolation de sécurité.
- ADR pour changement de frontière, format persistant, moteur graphique, nouveau
  `unsafe`, dépendance importante ou extension de permissions Tauri.
- Une capacité non implémentée renvoie un résultat documenté ; ne pas rendre
  silencieusement un document incomplet comme si le rendu était exact.
