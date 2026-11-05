---
name: review-diff
description: Utiliser pour relire un diff local ou une proposition de PR du projet PDF avant intégration.
---
# Relire un diff

Périmètre : $ARGUMENTS

Lire `CLAUDE.md`, les règles des chemins concernés et les exigences de qualité/sécurité.
Obtenir le diff et son contexte via les outils disponibles ; sinon demander à l'agent
principal le diff et la base exacte. Ne pas deviner la branche de comparaison.

Examiner dans cet ordre : perte de données, sécurité des entrées PDF et frontières IPC,
exactitude documentaire, mémoire/annulation, compatibilité, régressions et tests.
Pour chaque défaut actionnable, fournir fichier/ligne, scénario, effet observable et
correction proposée. Séparer constats prouvés, risques à vérifier et choix de style.

Signaler les commandes et OS réellement vérifiés. Si un test manque, proposer un cas
concret ; ne pas considérer un score de couverture comme une preuve de conformité.
Ne pas modifier les fichiers sauf demande de correction explicite. Aucun commit,
merge, commentaire distant ou message envoyé automatiquement.
