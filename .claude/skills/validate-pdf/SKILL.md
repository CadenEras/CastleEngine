---
name: validate-pdf
description: Utiliser pour diagnostiquer un PDF autorisé, une régression documentaire ou une divergence entre moteurs.
---
# Valider un cas PDF

Cas : $ARGUMENTS

1. Vérifier l'origine, l'autorisation d'accès et de conservation, puis limiter la taille
   du cas. Un document utilisateur reste privé ; aucun envoi à un validateur en ligne.
2. Lire `docs/QUALITY.md`, `docs/SECURITY.md` et `docs/COMMANDS.md`. Exécuter dans un
   environnement borné, hors réseau, avec les oracles et CLI réellement disponibles.
3. Vérifier structure (objets, xref, flux), sémantique (pages, texte, ressources), rendu
   et sauvegarde selon le défaut. Consulter la clause PDF pertinente dans une source
   autorisée ; ne pas attribuer une conclusion normative à une simple comparaison.
4. Comparer à au moins un outil indépendant pertinent, idéalement deux pour une
   divergence ambiguë. Documenter versions, options, limite et résultat de chaque outil.
5. Si le moteur/CLI n'existe pas, rendre un diagnostic structurel limité avec les outils
   disponibles et préciser que le moteur du projet n'a pas été testé.
   Si aucun outil pertinent n'est disponible, ne rendre aucun diagnostic structurel :
   énumérer les prérequis manquants et les contrôles non réalisés, sans conclure sur le PDF.
6. Produire un reproducer minimal redistribuable si possible, son hash et son résultat
   attendu. Ne pas copier le fichier privé dans le corpus public pour faciliter le test.

Sortie : capacités utilisées, anomalie reproduite ou non, preuves, contrôles non faits,
et proposition de régression. Un rectangle noir ne prouve pas le caviardage ; une image
de signature ne prouve pas une signature cryptographique ; un parseur tiers satisfait
ne prouve pas PDF/A ou PDF/UA.
