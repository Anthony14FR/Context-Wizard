# Context Wizard

[English](README.md) | **Français**

Donner à Claude Code le bon contexte de la base de code au bon moment, pour le moins de tokens possible, sur n'importe quel projet.

> **État : tout début de développement.** Context Wizard est en phase 0 de sa [feuille de route](#feuille-de-route). Rien n'est encore publié : les commandes d'installation ci-dessous fonctionneront à partir de la première version `0.x`.

## Pourquoi

Au début de chaque session sur une base de code moyenne ou grande, Claude dépense du temps et des tokens à redécouvrir les mêmes choses : l'arborescence, les manifestes, les points d'entrée, les modèles, les conventions. Context Wizard précalcule ce contexte, le maintient à jour automatiquement et le sert sous la forme la plus compacte, pour que l'exploration parte des bons fichiers et coûte moins cher.

## Fonctionnement

Context Wizard est un plugin Claude Code adossé à un unique binaire Rust, `wizard`. Le binaire s'exécute comme une commande de courte durée pour les hooks, et comme un serveur MCP de longue durée qui garde l'index du projet en mémoire.

Cette section décrit la conception visée pour la 1.0.

Le contexte est dévoilé progressivement, jamais d'un bloc :

| Niveau | Budget | Livraison | Contenu |
| --- | --- | --- | --- |
| L0 Aperçu | 800 tokens | Une fois, au début de la session | Stack, points d'entrée, commandes, rôle des dossiers |
| L1 Carte | 4 000 tokens | À la demande | Signatures classées avec numéros de ligne |
| L2 Requêtes | Par appel | Outils MCP | Recherche de symbole, références, tranche de tâche, impact |

Les fichiers et symboles sont classés par un PageRank personnalisé sur les liens d'import, de référence et de co-modification git. Le plugin apprend aussi de vos sessions : les fichiers lus puis modifiés gagnent du poids, le bruit s'efface, et les fichiers dont vous aurez probablement besoin sont suggérés à côté de votre prompt.

### Toutes les technologies

La prise en charge se dégrade progressivement, sans jamais échouer :

1. **Universel :** arborescence, tailles, fréquence de modification et co-modification git, apprentissage. Fonctionne sur tout projet.
2. **Syntaxe :** définitions, références et imports via tree-sitter, pour des centaines de langages.
3. **Lexical :** identifiants, titres et clés de premier niveau pour les fichiers sans grammaire.
4. **Recettes :** connaissance des frameworks (routes, modèles, composants) dans des fichiers TOML déclaratifs.
5. **LSP :** références exactes fournies par un serveur de langage gratuit, quand il est installé.

### Activation par projet

Au premier prompt dans un projet, Claude demande s'il faut activer Context Wizard. La réponse est enregistrée dans `.claude/wizard.json` : toutes les sessions suivantes, et tous les membres de l'équipe si le fichier est versionné, obtiennent le même comportement. Un projet qui répond non n'est plus jamais touché. Vous pouvez changer la décision avec `/context-wizard:on` et `/context-wizard:off`.

### Principes

- **Gratuit.** Licences permissives uniquement, aucune API payante, aucun service cloud, aucune télémétrie.
- **Local.** Le code, les prompts et les poids appris ne quittent jamais votre machine. Les données du projet vivent dans `.wizard/`, ignoré par git. Seule exception : un étalonnage facultatif du comptage de tokens, avec votre propre clé API Anthropic.
- **Inerte en cas d'échec.** Toute erreur, tout dépassement de délai ou tout binaire manquant laisse Claude Code fonctionner exactement comme sans le plugin.
- **À jour ou rien.** Chaque réponse est vérifiée par rapport aux empreintes des fichiers. Aucun contexte périmé n'est servi.

## Installation

Disponible à partir de la première version. Le dépôt est sa propre marketplace de plugins :

```sh
claude plugin marketplace add Anthony14FR/Context-Wizard
claude plugin install context-wizard@context-wizard
```

À la première utilisation, le plugin télécharge le binaire `wizard` adapté à votre plateforme depuis GitHub Releases et vérifie son empreinte SHA-256. Tant que le binaire n'est pas prêt, le plugin reste inerte.

Pour recevoir les mises à jour automatiquement, ouvrez `/plugin`, allez dans **Marketplaces** et choisissez **Enable auto-update**. Claude Code laisse cette option désactivée par défaut pour les marketplaces tierces.

Plateformes prévues : Linux, macOS et Windows, en x86_64 et aarch64.

## Feuille de route

| Phase | Contenu |
| --- | --- |
| P0 Fondations | Dépôt, CI, squelette du plugin, activation par projet, téléchargement du binaire |
| P1 Index, L0 et L1 | Scanner, cache d'empreintes, grammaires, extracteur lexical, estimateur de tokens |
| P2 Graphe et MCP | Classement, signaux git, serveur MCP, hooks |
| P3 Apprentissage | Signaux de session, association tâche-fichiers, auto-évaluation |
| P4 Recettes et LSP | Recettes Laravel, React, Next et Expo, repli sur LSP |
| P5 Distribution | Binaires publiés, marketplace communautaire, 1.0 |

Chaque phase ne se termine que lorsque ses critères de sortie sont atteints. L'objectif de la 1.0 : 60 % de tokens d'exploration en moins par rapport à Claude Code sans le plugin, avec un taux de réussite des tâches égal ou meilleur.

## Contribuer

Les contributions sont les bienvenues. Lisez [CONTRIBUTING.md](CONTRIBUTING.md) avant d'ouvrir une issue ou une pull request. Les issues, les pull requests et le code sont rédigés en anglais.

## Licence

Distribué au choix sous l'une des licences suivantes :

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- Licence MIT ([LICENSE-MIT](LICENSE-MIT))

Sauf mention contraire explicite de votre part, toute contribution que vous soumettez intentionnellement pour inclusion dans le projet, au sens de la licence Apache-2.0, est distribuée sous cette double licence, sans conditions supplémentaires.

En cas de divergence, la version anglaise fait foi.

Context Wizard est un projet indépendant, sans lien avec Anthropic et non approuvé par elle.
