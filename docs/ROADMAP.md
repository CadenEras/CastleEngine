# Phasage par capacité vérifiable

Pas d'échéances artificielles : chaque phase se termine par des preuves. Les numéros
de phase ne sont pas les versions SemVer ; le support PDF est une matrice de capacités,
pas un booléen « compatible PDF 2.0 ».

| Phase | Livrable | Critère de sortie |
| --- | --- | --- |
| P0 — fondations | Workspace, UI vide, protocole worker, budgets, corpus, CI OS | Build natif des trois OS, contrôle annulation/worker, toolchains verrouillées et baseline mémoire |
| P1 — lecture structurée | Tokenizer, objets, xref classique, sources, pages ; CLI diagnostic | Cas valides et invalides, limites et références cycliques testés ; refus explicite des capacités absentes |
| P2 — lecteur utile | Filtre Flate, xref/object streams, interpréteur graphique minimal, rendu tuilé, polices/images ciblées, navigation (ordre : ADR-0003) | Première page rendue, UI réactive, corpus/goldens et comparaisons, mémoire/worker contrôlés |
| P3 — compatibilité et texte | Filtres secondaires, texte/ToUnicode, recherche/sélection | Matrice de support publiée, tests sur corpus hétérogène, lecture et copie Unicode vérifiées |
| P4 — écriture et pages | Writer, rotation/réordre/suppression, fusion/découpe, sauvegarde | Réouverture indépendante, erreurs d'écriture sans perte, propriétés de round-trip et undo/redo |
| P5 — annotations/formulaires | Annotations, apparences, AcroForm simple, impression selon OS | Réouverture dans moteurs indépendants, valeurs/apparences conservées, accessibilité UI testée |
| P6 — fonctions avancées séparées | OCR, édition de texte existant, chiffrement, signatures, caviardage | Étude et tests propres à chaque fonction ; aucune promesse de sécurité avant validation dédiée |
| Stabilisation — diffusion | Packaging, signatures OS, mises à jour, guide utilisateur | Release depuis tag testé, corpus accepté, mesures et limites publiques, récupération après erreur |

## Première tranche concrète pour Claude

Lire ce kit puis proposer l'ADR P0 avec les versions vérifiées à ce moment-là.
Créer le workspace, un shell Tauri minimal et un worker qui traite une requête bornée
et annulable sans lire de PDF réel. Ajouter CI Rust/UI/native, fixture synthétique et
mesure du plancher mémoire. Ne pas commencer par une barre d'outils Adobe complète.

## Décisions à enregistrer en P0

OS/architectures minimums, versions Rust/Node/Tauri/React, politique de codecs et licences,
protocole et isolation du worker par OS, profil PDF initial et rendu des polices,
machines de mesure et budgets. Utiliser `docs/templates/ADR.md` et marquer les choix
proposés comme tels jusqu'à validation dans le dépôt.

L'application vise les fonctions basiques d'un éditeur PDF progressivement. La fidélité
des polices, transparences, couleurs, impressions et éditions de texte est un chantier
à part entière ; aucune phase courte ne garantit une parité générale avec Acrobat.
