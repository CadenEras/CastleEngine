---
name: implement-feature
description: Utiliser pour une tranche fonctionnelle ou un correctif du moteur PDF ou de son interface, avec un résultat vérifiable.
---
# Implémenter une tranche

Demande : $ARGUMENTS

1. Lire `CLAUDE.md`, l'état Git, `docs/ROADMAP.md` et les règles des chemins concernés.
   Identifier ce qui existe ; les commandes cibles ne sont pas forcément installées.
2. Définir le résultat observable, le périmètre et les cas d'échec. Si la demande est
   claire, avancer ; poser seulement une question dont la réponse change le résultat.
3. Consulter `architect` ou `pdf-specialist` pour une frontière ou une sémantique
   incertaine. Une petite correction ne demande pas une équipe complète.
4. Ajouter le test pertinent, reproduire le défaut si applicable, puis implémenter.
   Respecter budgets, annulation et données non fiables. Un simple changement de
   documentation/configuration reçoit une validation proportionnée.
5. Exécuter les contrôles existants de `docs/COMMANDS.md` pour ce périmètre. En cas
   d'absence, la signaler ; ne pas inventer une commande ni marquer le test réussi.
6. Lire le diff. Pour parsing, écriture, `unsafe`, droits ou IPC sensibles, faire
   relire par `reviewer` avec les chemins et exigences utiles.
7. Rendre résultat, changements, commandes/résultats, limites et prochaine étape utile.
   Un test vert de l'UI ne démontre ni le moteur ni le packaging natif.

Ne pas créer de commit, pousser, publier ou fusionner sauf instruction correspondante.
L'invocation autorise les modifications locales nécessaires, pas un déploiement.
