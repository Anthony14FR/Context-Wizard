# Context Wizard

[English](README.md) | **Français**

Donner à Claude Code le bon contexte du code au bon moment, pour le moins de tokens possible, sur n'importe quel projet.

> **Statut : avant la première version.** Toutes les phases de la [feuille de route](#feuille-de-route) jusqu'aux recettes et au LSP sont implémentées. La première version `0.x` et le benchmark public arrivent ensuite ; les commandes d'installation ci-dessous fonctionnent à partir de cette version.

## Pourquoi

Au début de chaque session sur une base de code moyenne ou grande, Claude passe du temps et des tokens à redécouvrir les mêmes choses : l'arborescence, les manifestes, les points d'entrée, les modèles, les conventions. Context Wizard précalcule ce contexte, le garde à jour automatiquement et le sert sous la forme la plus compacte, pour que l'exploration parte des bons fichiers et coûte moins.

## Ce qu'il fait

- **Vue d'ensemble au démarrage.** Environ 800 tokens sur la stack, les sous-projets, les points d'entrée, les commandes, le rôle des dossiers et les faits du framework, injectés avant le premier prompt.
- **Fichiers prédits à chaque prompt.** Les cinq fichiers dont une tâche a le plus probablement besoin, avec leurs plages de lignes, injectés seulement quand la prédiction est sûre.
- **Des réponses plutôt que des recherches.** Un serveur MCP répond aux questions de structure à partir d'un index rafraîchi avant chaque réponse.
- **Des greps qui se répondent.** Quand Claude cherche un symbole connu avec Grep, sa définition est jointe à la recherche.
- **Apprentissage.** Chaque session apprend quels fichiers ont servi pour quel type de tâche. Les prédictions s'évaluent à la fin de chaque réponse et ajustent la confiance qu'elles accordent à l'apprentissage.

### Outils MCP

| Outil | Répond à |
| --- | --- |
| `overview` | Stack, sous-projets, points d'entrée, commandes et rôle des dossiers |
| `map` | Les définitions les plus utilisées, avec signatures et numéros de ligne, en 4 000 tokens |
| `find_symbol` | Où un nom est défini |
| `refs` | Où un nom est utilisé |
| `exact_refs` | Les références exactes d'un serveur de langage, quand il y en a un d'installé |
| `slice` | Les fichiers les plus utiles pour une tâche décrite en mots |
| `impact` | Ce qu'une modification de fichier peut toucher : dépendants, fichiers modifiés ensemble, fichiers ouverts ensemble |
| `explain` | Le plan d'un fichier sans le lire |
| `facts` | Les faits du framework : routes, modèles et relations, schéma, composants, hooks, pages, écrans |

### N'importe quelle technologie

Le support se dégrade proprement et n'échoue jamais :

1. **Universel :** arborescence, tailles, churn et co-modifications git, apprentissage. Fonctionne sur tout projet.
2. **Syntaxe :** définitions, références et imports via tree-sitter, pour des centaines de langages. Les grammaires se téléchargent à la première utilisation.
3. **Lexical :** identifiants, titres et clés de premier niveau pour les fichiers sans grammaire.
4. **Recettes :** la connaissance des frameworks dans des fichiers TOML déclaratifs. Laravel, React, Next et Expo sont fournis, et vous pouvez [écrire les vôtres](docs/recipes.md).
5. **LSP :** les références exactes de rust-analyzer, typescript-language-server, pyright, pylsp, intelephense, gopls ou clangd, quand ils sont installés.

### Activation par projet

Au premier prompt dans un projet, Claude demande s'il faut activer Context Wizard. La réponse est enregistrée dans `.claude/wizard.json`, pour que chaque session suivante, et chaque coéquipier une fois le fichier commité, ait le même comportement. Un projet qui répond non n'est jamais indexé.

### Principes

- **Gratuit.** Licences permissives uniquement, aucune API payante, aucun service cloud, aucune télémétrie.
- **Local.** Le code, les prompts et les poids appris ne quittent jamais votre machine. Les données du projet vivent dans `.wizard/`, qui s'ignore lui-même dans git.
- **Inerte en cas d'échec.** Une erreur, un délai dépassé ou un binaire absent laissent Claude Code fonctionner exactement comme sans le plugin.
- **À jour ou rien.** L'index est rafraîchi d'après les empreintes des fichiers avant les réponses. Un contexte périmé n'est jamais servi.

## Installation

Le dépôt est sa propre marketplace de plugins :

```sh
claude plugin marketplace add Anthony14FR/Context-Wizard
claude plugin install context-wizard@context-wizard
```

Au démarrage de session suivant, le plugin télécharge le binaire `wizard` de votre plateforme depuis les GitHub Releases dans `${CLAUDE_PLUGIN_DATA}/bin`, vérifie son empreinte SHA-256 et l'installe de façon atomique. Tant que le binaire n'est pas prêt, le plugin reste inerte. Des binaires existent pour Linux, macOS et Windows, en x86_64 et aarch64, sans dépendance d'exécution.

Pour recevoir les mises à jour automatiquement, ouvrez `/plugin`, allez dans **Marketplaces** et choisissez **Enable auto-update**. Claude Code laisse cette option désactivée par défaut pour les marketplaces tierces. Sinon, Context Wizard affiche une ligne d'avis quand une nouvelle version sort.

### Canal bêta

Les préversions sortent d'abord sur une seconde marketplace, `context-wizard-beta`, construite depuis la branche `next`. Pour les essayer, désinstallez d'abord le plugin stable, car les deux exécuteraient leurs hooks :

```sh
claude plugin uninstall context-wizard@context-wizard --keep-data
claude plugin marketplace add Anthony14FR/Context-Wizard#next
claude plugin install context-wizard@context-wizard-beta
```

Pour revenir en arrière, désinstallez `context-wizard@context-wizard-beta` et réinstallez `context-wizard@context-wizard`. Les données de projet sont communes aux deux canaux.

## Utilisation

Répondez à la question au premier prompt d'un projet, puis travaillez comme d'habitude. Commandes :

| Commande | Effet |
| --- | --- |
| `/context-wizard:on` | Active Context Wizard pour ce projet |
| `/context-wizard:off` | Le désactive ; Claude Code ne charge plus le plugin dans ce projet |
| `/context-wizard:status` | Décision, emplacement du binaire et dernière erreur d'installation |
| `/context-wizard:stats` | Précision des prédictions dans le temps, fichiers les plus utiles, glossaire |
| `/context-wizard:purge` | Supprime l'index et les données apprises, ou avec `all` la décision aussi |

Le binaire se lance aussi depuis un terminal, pour des scripts et des vérifications : `wizard index`, `wizard map --level 0|1`, `wizard stats`, `wizard recipes`, `wizard purge`, chacun avec `--project <chemin>`.

## Configuration

Context Wizard ne demande aucune configuration. Ces réglages existent si vous en avez besoin :

| Réglage | Effet |
| --- | --- |
| `DISABLE_UPDATES`, `DISABLE_AUTOUPDATER` ou `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | Pas de vérification quotidienne des versions |
| `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY` | Utilisés pour le téléchargement du binaire et la vérification des versions |
| `.claude/context-wizard/recipes/<nom>/` | Recettes du projet, voir [le guide des recettes](docs/recipes.md) |
| `wizard calibrate` avec `ANTHROPIC_API_KEY` | Mesure les octets par token de ce projet avec l'endpoint gratuit `count_tokens` d'Anthropic, pour que les budgets de tokens collent au vrai tokenizer. Il envoie un échantillon de fichiers, jamais des fichiers contenant des secrets |

Context Wizard n'écrit que ces fichiers dans un projet :

- `.claude/wizard.json` : la décision, faite pour être commitée.
- `.claude/settings.local.json` : une seule entrée `enabledPlugins`, seulement quand le projet répond non.
- `.wizard/` : l'index, la base d'apprentissage et ses sauvegardes. Il contient un `.gitignore` qui ignore tout.

## Dépannage

| Symptôme | Vérification |
| --- | --- |
| Claude ne pose jamais la question | Lancez `/context-wizard:status`. Si le binaire manque, la dernière erreur d'installation en donne la raison ; le téléchargement est retenté à chaque démarrage de session. |
| Le téléchargement échoue derrière un proxy | Définissez `HTTPS_PROXY` (et `NO_PROXY` pour les hôtes internes) dans l'environnement d'où Claude Code démarre. |
| Rien ne se passe après une mise à jour | Les hooks restent inertes pendant le téléchargement du binaire de la nouvelle version. Démarrez une nouvelle session ou lancez `/reload-plugins`. |
| Windows SmartScreen ou macOS Gatekeeper | Le lanceur télécharge avec `curl` ou PowerShell, qui ne marquent pas le fichier comme venant d'Internet, donc aucun des deux ne l'inspecte. Si vous copiez le binaire à la main depuis un navigateur, débloquez-le dans ses propriétés ou lancez `xattr -d com.apple.quarantine` dessus. |
| `exact_refs` se rabat sur les références par nom | Aucun serveur de langage pour ce langage n'est sur le `PATH` ; la réponse indique lequel installer. |
| Une recette de projet ne fait rien | Lancez `wizard recipes --project .` : il liste où chaque recette s'applique et chaque problème des recettes cassées. |
| Réponses périmées ou fausses | Lancez `/context-wizard:purge` ; l'index est reconstruit à la session suivante. |

## Désinstallation

```sh
claude plugin uninstall context-wizard@context-wizard
```

Désinstaller depuis la dernière portée supprime aussi `${CLAUDE_PLUGIN_DATA}`, avec les binaires téléchargés, sauf si vous passez `--keep-data`. Les données de projet restent dans chaque projet : lancez d'abord `/context-wizard:purge all` dans chacun, ou supprimez vous-même `.wizard/` et `.claude/wizard.json`.

## Feuille de route

| Phase | Contenu | État |
| --- | --- | --- |
| P0 Fondations | Dépôt, CI, squelette du plugin, activation, amorçage du binaire | Fait |
| P1 Index, L0 et L1 | Scanner, cache d'empreintes, grammaires, extracteur lexical, estimateur de tokens | Fait |
| P2 Graphe et MCP | Classement, signaux git, serveur MCP, hooks | Fait |
| P3 Apprentissage | Signaux de session, correspondance tâche vers fichiers, auto-évaluation | Fait |
| P4 Recettes et LSP | Recettes Laravel, React, Next et Expo, repli LSP | Fait |
| P5 Distribution | Binaires publiés, marketplace communautaire, benchmark public, 1.0 | En cours |

L'objectif de la 1.0 est 60 % de tokens d'exploration en moins que Claude Code sans le plugin, avec un taux de réussite des tâches égal ou meilleur, mesuré par le [benchmark](bench/README.md) sur six projets publics.

## Sécurité

Voir [SECURITY.md](SECURITY.md) pour signaler une vulnérabilité, et le [modèle de menaces](docs/threat-model.md) pour ce que Context Wizard protège et comment.

## Contribuer

Les contributions sont les bienvenues. Lisez [CONTRIBUTING.md](CONTRIBUTING.md) avant d'ouvrir une issue ou une pull request. Les issues, les pull requests et le code sont rédigés en anglais. Les décisions d'architecture sont consignées dans [docs/adr](docs/adr/README.md).

## Licence

Sous licence, au choix :

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- licence MIT ([LICENSE-MIT](LICENSE-MIT))

Sauf mention contraire explicite de votre part, toute contribution soumise intentionnellement pour inclusion dans ce travail, au sens de la licence Apache-2.0, est sous double licence comme ci-dessus, sans conditions supplémentaires.

Context Wizard est un projet indépendant, ni affilié ni approuvé par Anthropic.
