---
name: prepare-release
description: Utiliser pour préparer les versions, notes et vérifications précédant une release du projet PDF.
---
# Préparer une release

Version proposée : $ARGUMENTS

Lire `docs/GIT.md`, `docs/QUALITY.md` et `docs/SECURITY.md`. Cette procédure est
invocable automatiquement comme aide de préparation ; elle ne publie rien.

1. Vérifier branche, état de travail, version actuelle, changement de contrat et
   cohérence SemVer. Refuser une référence publiée à déplacer.
2. Préparer les modifications locales de versions Cargo/Tauri/package et lockfiles,
   changelog, compatibilité, limitations connues, migrations et notes de release.
   Si l'application n'existe pas encore, préparer la liste de prérequis au lieu
   d'inventer des manifests ou une release prête.
3. Exécuter les contrôles disponibles et collecter les preuves OS, corpus, performance,
   audits et artefacts. Un contrôle absent reste un prérequis non satisfait.
4. Fournir le diff, la version proposée, le commit/base, les preuves et la liste des
   blocages. Préparer les commandes exactes de tag/publication pour revue, sans les lancer.

Ne pas fusionner, créer/pousser un tag, signer des binaires ou publier sans une demande
explicite couvrant ces actions. Si cette autorisation existe déjà, suivre la procédure
de release avec les protections effectives ; ne pas redemander la même autorisation.
Ne jamais lire une clé privée pour « vérifier » qu'elle existe.
