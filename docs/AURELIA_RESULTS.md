# Présentation des résultats Aurelia

## Présentation actuelle

Le tableau Aurelia reprend la composition de la maquette PNG : en-tête avec
profil et lancement, cinq cartes (global, CPU, GPU, mémoire, stockage), résumé
avec résultats par test, radar comparatif, FPS et comparaison rapide, puis
configuration testée. Les grilles adaptent leurs colonnes à la largeur visible ;
le contenu reste défilant verticalement. La barre GTK native est conservée. Les six destinations principales sont placées
sous cette barre ; la colonne gauche contient les sous-menus contextuels. Aurelia
possède une seule rangée Résumé / Détails / Graphes / Comparaison sous les scores.

Les accents Aurelia sont ambrés (#F59E0B). `hardware_colors.rs` centralise la
sélection automatique AMD/Intel/neutre ; mémoire et stockage utilisent
des accents bleu acier et vert sobre distincts. Aucun matériel n’est codé en dur.

Le profil complet utilise désormais `aurelia-system-v3-storage` (schéma 3).
Les charges CPU, RAM, cache, Vulkan et bibliothèques de v2 sont réutilisées
sans changement. Le stockage est une phase supplémentaire. La CLI historique
`--evaluate` reste v2 ; `--evaluate-storage` sélectionne explicitement v3.
Les profils CPU avancé et GPU Vulkan restent indépendants, accessibles depuis
le sélecteur de profil et accompagnés d’un retour direct aux résultats Aurelia.

Le dernier résultat s’ouvre automatiquement ; une session précédente compatible
est recherchée parmi les seize précédentes du même protocole, en privilégiant
une référence conservée lorsqu’elle est sélectionnée. L’historique garde ses
actions de consultation, comparaison A/B, référence, import, export et archivage.
L’enregistrement est automatique. Les JSON importés/exportés conservent les
mesures originales. Aucun historique n’est migré, écrasé ou supprimé.

## Points absolus : aurelia-rate-points-v1

Les entrées sont normalisées par une constante de débit de référence égale à
une unité par seconde, et non par un modèle de processeur ou GPU :

| Catégorie | Mesures réellement utilisées | Formule et constante |
| --- | --- | --- |
| CPU | Étapes/s monothread et multithread soutenu, époques canonical64 | sqrt(mono × multi), K = 1 point par étape/s |
| GPU | Images/s du raster système hors écran | K × FPS, K = 1 point par image/s |
| Mémoire | Lecture, écriture, copie en Go/s logiques | racine cubique(lecture × écriture × copie), K = 1 point par Go/s logique |
| Stockage | Lecture vérifiée et écriture synchronisée en Go/s | sqrt(lecture × écriture), K = 1 point par Go/s |

Chaque débit est la moyenne arithmétique des trois répétitions du protocole.
Les moyennes géométriques sont calculées dans le domaine logarithmique.
Global = (CPU × GPU × mémoire × stockage)^(1/4), poids 25 % chacun.
Une catégorie doit être exécutée et contenir tous ses sous-tests valides.
Une catégorie manquante ou échouée n’est jamais remplacée par zéro ; aucun
global n’est attribué à un parcours incomplet ni à un ancien protocole.
Doubler tous les débits double tous les points, sans plafond. Les nombres de
la maquette ne sont pas reproduits et aucun multiplicateur flatteur n’est ajouté.
Les valeurs de catégories ne mesurent pas leur importance relative ; leur
agrégation est une convention explicite, versionnée, pas une grandeur physique.
Les nombres de threads disponibles et les tailles RAM adaptatives restent ceux
du protocole existant. Aucune modification de charge ne vise à améliorer un score.

Les tests système CPU/GPU/RAM sont à durée cible, avec quantité de travail
variable : ils utilisent un débit, jamais K / durée totale. Le CPU avancé peut
calibrer le nombre d’étapes ; son temps total n’est donc pas un score comparable.
Le stockage réalise une quantité fixe d’octets : son débit est octets / temps.
Les cas avancés CPU, le raster fenêtré, le cache, la charge combinée, glibc et les
bibliothèques conservent leurs mesures et comparaisons propres. Ils ne sont pas
comptés une seconde fois dans le global. Aucun test GPU compute n’est inventé.

Le JSON v3 conserve les mesures brutes, paramètres, identités matérielles,
version applicative, empreintes des binaires et protocoles. `parameters.rate_scores`
contient la version, la méthode, les constantes, les scores par catégorie et le
global. À l’import, les points sont vérifiés contre les mesures ; les données
aplaties doivent correspondre exactement aux catégories. Les points affichés
sur v2 sont une dérivation en lecture seule des catégories disponibles ; aucun
recalcul ne réécrit un historique original. Une future formule doit porter une
nouvelle version ; aucune migration silencieuse des résultats n’est prévue.

## Épreuve de stockage direct-sequential-v1

- Cible : volume contenant le dossier d’état GameShift, pas nécessairement le
  disque système. Le volume, le dossier, le type de FS et les paramètres sont
  enregistrés ; aucun modèle physique n’est déduit d’un volume complexe.
- Fichier neuf privé (0600, create_new, O_NOFOLLOW), immédiatement dissocié de son
  nom après ouverture : seul son descripteur le conserve jusqu’à la fermeture.
  Une annulation ou un plantage libère aussi ce fichier. Aucun fichier existant
  ni périphérique brut n’est ouvert en écriture.
- Trois passages de 256 Mio, blocs alignés de 1 Mio, O_DIRECT ; écritures suivies
  de fdatasync inclus dans le temps. Trois lectures avec vérification complète
  du motif déterministe, incluse dans le temps. Motif pseudo-aléatoire par bloc.
- Réserve de 512 Mio libres en plus du fichier ; FS mémoire refusés. Si O_DIRECT,
  la synchronisation ou la vérification échoue, aucun débit de remplacement.
- O_DIRECT contourne le cache de pages Linux, pas les caches du SSD/contrôleur,
  de la VM ou d’un stockage réseau. Compression/déduplication du FS et répétition
  du même bloc peuvent aussi influer sur les résultats. Il s’agit de performances
  séquentielles du chemin de stockage, pas d’IOPS aléatoires ni d’endurance.
  La vérification CPU peut limiter la lecture sur un stockage très rapide.
- Budget d’écriture par exécution : 768 Mio logiques. La phase supplémentaire
  peut allonger sensiblement le benchmark sur un disque lent ; aucun délai
  artificiel ni réduction des autres phases n’est ajouté.

## Comparabilité et graphiques

Les nouveaux points comparent seulement des protocoles et binaires identiques,
des unités et descriptions de travail compatibles. Le mode intermachines est
explicite : il reprend les règles existantes pour les workers CPU et l’identité
GPU, tout en conservant les paramètres de rendu. Une taille RAM différente peut
rendre la catégorie et le global incomparables. Le stockage exige le même mode,
volume de travail et taille de bloc ; changer de disque est précisément un cas
de comparaison permis. Les écarts sont affichés en points et en pourcentage.

Le radar montre les quatre rapports de points B/A, A = 100, avec une échelle
commune explicitement indiquée et adaptée sans plafonner les valeurs. Sans les
quatre catégories compatibles, il explique son indisponibilité. La courbe FPS
montre les intervalles réels du premier passage GPU, dans l’ordre des images,
avec une enveloppe min/max par colonne pour conserver les pics. Le rendu est
hors écran : ce n’est pas un horodatage d’affichage physique ni une capture de
jeu. Les tests affichent leur mesure brute et une barre de comparaison lorsque la
référence est compatible. Sans référence, la barre est indisponible. La variabilité
reste accessible au survol et dans les détails. Aucun axe ou échantillon n’est rempli avec une valeur fictive.

## Validation manuelle à effectuer sur la machine cible

1. Ouvrir l’AppImage en 1920×1080 puis 2560×1440 ; déplacer entre les deux écrans,
   redimensionner et vérifier le retour à la ligne, les accès aux outils, défilement et
   contrôles natifs de fenêtre. Les captures GTK sont disponibles via `--capture-aurelia DOSSIER` ; elles ne
   lancent aucun benchmark et ne changent aucun réglage matériel.
2. Lancer le profil complet, vérifier le volume affiché, les températures et les
   conditions dans les détails. Vérifier l’absence de global si le GPU manque.
3. Faire un second passage, ouvrir Historique, consulter et comparer A/B ; vérifier
   les points, pourcentages et réserves de variabilité.
4. Exporter puis importer sur une autre machine avec la même AppImage ; activer
   explicitement le mode intermachines. Vérifier le refus des charges différentes.
5. Consulter/importer un ancien v1/v2 : aucune suppression ni conversion de ses
   mesures, aucun global incluant un stockage absent.

## Indices existants

Les valeurs proviennent de `gameshift-system-bench::system::reporting`, méthode
`aurelia-system-relative-v2`, et ne sont pas recalculées par un nouveau moteur UI.

Pour chaque débit compatible : indice = 100 × débit B / débit A.
Une catégorie est la moyenne géométrique de ses indices d'épreuves.
L'indice matériel est la moyenne géométrique des cinq catégories obligatoires :
CPU, cache, RAM, GPU et charge combinée, à poids égaux entre ces catégories.
Le groupe applicatif et la synthèse historique restent dans le rapport complet.

La référence vaut 100 ; l'écart affiché vaut indice − 100, en pourcentage.
Un indice de 120 signifie un indice supérieur de 20 % à cette référence ; ce ne
sont ni 120 points absolus ni un remplissage de capacité. Les références doivent
être identifiées et identiques pour interpréter plusieurs indices ensemble.

Les critères existants de protocole, empreintes binaires, travail, unités et
couverture restent appliqués. Le mode intermachines doit être choisi explicitement.
Une référence incompatible ne produit pas d'indice. Une catégorie absente ne
produit pas zéro et empêche l'indice matériel complet. Les mesures compatibles
restent consultables. Les anciens parcours partiels restent lisibles avec leur
rapport et leur méthode historiques ; ils ne deviennent pas un score complet.

La variation est le coefficient de variation entre répétitions. Les notices
« À confirmer » reprennent les limites des mesures et du comparateur existants ;
elles ne représentent pas un test de significativité statistique.

## Unités et graphiques

Les mesures brutes conservent leurs unités originales. La RAM représente les
Go/s logiques des épreuves existantes, pas un compteur du bus DRAM. Les FPS du
raster sont ceux de la scène synthétique, pas une prédiction des jeux ni une date
d'affichage physique. Les indices ne mélangent pas directement ces unités.

Les barres A/B utilisent une même échelle affichée pour toutes les catégories de
la comparaison ; elles peuvent dépasser 100. Les tracés CPU et frametimes
existants sont conservés. Les cartes se répartissent dans une grille adaptative,
sans imposer trois colonnes lorsque la largeur disponible ne le permet pas.

Les rapports techniques complets, avertissements, capteurs enregistrés et profils
observés restent accessibles. Une métadonnée absente est indiquée comme non
enregistrée ; elle n'est pas remplacée par l'état actuel de la machine.

## Vérifications

Tests du modèle de présentation :
- aucune notation sans référence, même avec des mesures valides ;
- indices identiques à ceux du moteur existant ;
- résultat partiel sans score matériel ni zéro pour le GPU manquant ;
- référence binaire incompatible avec raison explicite ;
- conservation de toutes les épreuves, au-delà des seize anciennes cartes.

Suites à exécuter :

```sh
# Utiliser un environnement Rust/GTK/Vulkan déjà configuré.
cargo test -p gameshift-gui --offline --bin gameshift-gui
cargo test -p gameshift-system-bench --offline --test comparison --test system_v2
```

Validation graphique manuelle en 1080p et 1440p :
1. Ouvrir un rapport système enregistré, sans référence, puis avec une référence
   conservée compatible. Vérifier la synthèse et les catégories.
2. Comparer deux sessions existantes ; vérifier A = 100, les écarts et la graduation.
   Essayer un rapport incompatible et vérifier que l'ancien résultat comparé ne
   reste pas affiché comme résultat de cette tentative.
3. Consulter un ancien parcours partiel et ouvrir son rapport complet.
4. Vérifier les détails, exports et graphiques CPU/raster lors du prochain test
   voulu par l'utilisateur ; aucun benchmark lourd n'est nécessaire à la simple
   consultation d'un historique.
5. Réduire la fenêtre et changer d'écran : vérifier le retour à la ligne des cartes
   et l'accessibilité des rapports repliables.

## Vérifications réalisées le 12 novembre 2026

- 73 tests réussis : GUI 42 (une sonde I/O explicitement ignorée), bibliothèque
  système 10, comparaison/historique 8, protocole système 13.
- Clippy GUI et système avec `-D warnings` : aucun avertissement.
- Essai réel de stockage sur le volume ext4 de la VM de développement : trois
  écritures et lectures de 256 Mio vérifiées ; aucun fichier de travail restant.
  Refus confirmé sur /tmp en mémoire. Ces débits de VM ne sont pas des références
  matérielles et ne sont pas injectés dans l’historique de l’application.
- Export/import v3 conserve les octets originaux ; scores incohérents et
  protocoles incompatibles rejetés. Historiques v1/v2 couverts par les tests.
- AppImage construite avec audit ABI du script existant ; SHA-256 et diagnostic
  JSON sans interface vérifiés. Présentation 1080p/1440p et mesures Radeon à
  valider manuellement sur la machine cible selon la procédure ci-dessus.

## Correction graphique : validation du 12 novembre 2026

Référence unique examinée : `~/Dev/Tableau de bord Aurelia Benchmarks.png`.
Une capture de la présentation précédente a été produite avant modification,
faute de capture récente de l’utilisateur dans Dev. La comparaison a porté sur
la vraie fenêtre GTK, avec sa barre native et son menu principal conservés.

| Élément de la référence | Composant corrigé |
| --- | --- |
| En-tête et lancement | Titre 29 px, sélection de profil et action ambrée ; conditions à la demande |
| Grand score global | Carte double largeur sur bureau, chiffre 64 px, état indisponible conservé |
| CPU, GPU, mémoire, stockage | Quatre cartes, chiffres 34 px, matériel et évolution compatible ; aucune formule sur la carte |
| Trois colonnes | Grille 5/4/3 : tests / graphiques / résultat et comparaison |
| Graphique FPS | Tracé 240 px, axes FPS/image, origine zéro, min/max/moyenne du premier passage |
| Historique et rapports | Action séparée avec retour aux résultats ; détails techniques et rapport repliables |

Les trois rangées de navigation horizontale ont été supprimées du résumé.
Les laboratoires CPU/GPU et l’ancienne page d’outils restent accessibles depuis
le profil et une action de retour. Les calculs, constantes, protocoles, conditions
de compatibilité, données historiques et contrôles matériels sont inchangés.

Palette centralisée dans `style.css` : GameShift #F59E0B, AMD #ED1C24,
Intel #0071C5, autres fabricants neutres, mémoire #8CA9BE, stockage #539977.
Le choix des classes fabricantes reste dans `hardware_colors.rs`. Les couleurs
de fabricants ne sont pas utilisées pour qualifier le succès d’une session.

La grille tient compte du bord réellement visible de la fenêtre pour éviter
qu’une largeur minimale du contenu empêche le retour à la ligne. Le global
passe au-dessus des quatre catégories sous 1150 px utiles ; les catégories
passent par paires sous 700 px. Le corps utilise trois colonnes à partir de
1200 px utiles, deux à partir de 850, puis une seule. Les graphiques ne sont
pas étirés artificiellement pour remplir la hauteur de la fenêtre.

Validation : 44 tests GUI réussis, une sonde I/O ignorée intentionnellement ;
Clippy avec `-D warnings` sans avertissement. La moyenne FPS affichée utilise
le nombre d’images divisé par la somme de leurs durées, et non la moyenne
arithmétique des FPS instantanés. Cela ne modifie aucun score de benchmark.
Les captures finales se trouvent dans `design/validation/aurelia-correction-20261112/`.
Elles couvrent 1920×1080, 2560×1440 et une fenêtre réellement allouée de 1182×800
(1100×800 demandés, minimum courant de la fenêtre native). Le mode de capture
contrôle les cinq cartes, l’absence de barre d’onglets visible et les débordements
horizontaux des cartes/panneaux.

Limite explicite : aucun historique matériel réel n’est présent dans cette VM.
Les captures représentent l’état initial, pas un résultat inventé. Le rendu des
courbes avec des résultats réels Radeon, les libellés du matériel et les écarts
A/B restent à vérifier sur la machine de l’utilisateur. La session Wayland de
la VM permet le rendu GTK mais n’expose pas de GPU Radeon ; ses avertissements
EGL de pilote virtuel ne constituent pas une validation matérielle.

## Reproduction de la maquette : bandeau et glossy (12 novembre 2026)

Cette passe suit la demande de reproduction de `Tableau de bord Aurelia Benchmarks.png`,
y compris le déplacement de la navigation générale, désormais explicitement autorisé.
Aucun pourcentage de ressemblance n’est revendiqué : la barre GTK, le contenu réel
et les unités du protocole imposent des différences avec l’illustration.

- `workspace_nav.rs` place les six destinations **existantes** (Accueil, GPU, CPU,
  Benchmarks, Profils, Réglages) sous la barre native. Aucun module Jeux fictif
  n’est ajouté. Le sous-menu suit la destination : tableau de bord et diagnostics,
  GPU, CPU, Aurelia/historique/comparaison/protocole/configuration, profils, réglages.
- Le sous-menu Aurelia utilise les espaces existants ; les laboratoires avancés
  restent accessibles par le profil. L’historique, le lancement, l’annulation,
  l’import/export, les références et l’archivage sont conservés.
- Les boutons reprennent un reflet supérieur, une transition au milieu du dégradé,
  une bordure éclairée et une ombre intérieure basse. Les cartes ont un fond
  légèrement teinté selon le matériel, et pas seulement un contour coloré.
- Les proportions des scores sont 3/11 pour le global et 2/11 par catégorie.
  Le corps suit 45/30/25 % : tests / graphiques / comparaison. Le global utilise
  un laurier vectoriel et un chiffre de 52 px ; les autres scores sont à 31 px.
- Le résumé conserve une seule rangée de quatre vues, comme dans la référence.
  Les graphes agrandis sont dans Graphes ; les formules et unités brutes dans
  Détails. Sans données ni référence compatible, les états restent indisponibles.
- Le CPU est affiché en **ms/cycle de simulation** dans le résumé :
  temps équivalent = 1000 / débit moyen enregistré. Ce n’est pas le temps total
  du benchmark ni la latence d’une instruction. La variabilité affichée concerne
  les mesures brutes, pas une nouvelle campagne de temps. Les débits GPU et RAM
  gardent leurs unités. Le JSON conserve `étapes/s`, les valeurs et les scores
  d’origine pour assurer la compatibilité ; aucune charge ni formule n’est modifiée.
- La décoration ambrée du sous-menu est un dessin vectoriel, sans mesure représentée.
  La barre de titre conserve les contrôles et le comportement GTK natif.

Validation : 45 tests GUI réussis, une sonde I/O ignorée volontairement ; Clippy
avec `-D warnings` sans avertissement. Captures natives du binaire release dans
`design/validation/aurelia-maquette-glossy-20261112/` : 1920×1080, 2560×1440,
1182×800, avec cinq cartes, une seule barre de vues Aurelia et aucun débordement
horizontal de cartes/panneaux détecté. Une seconde série conserve les neuf vues
existantes après déplacement des onglets. Les captures ont été ouvertes et comparées
à la référence, notamment pour les reflets, les proportions et la disposition.

Les captures restent celles d’un environnement sans historique Radeon réel :
elles n’inventent aucun score ni courbe. Il reste à contrôler les résultats réels,
les libellés et les comparaisons sur la machine cible, ainsi que les clics de
lancement/export et le déplacement entre écrans physiques. La VM émet des
avertissements EGL liés à son pilote virtuel ; un avertissement transitoire de
largeur du contenu Accueil a été relevé lors du changement de page de la série
réduite, sans débordement détecté dans les captures Aurelia finales.

## Implémentation de la proposition validée (12 novembre 2026)

Référence validée explicitement par l’utilisateur : `Proposition-Aurelia-identite-v1.png`,
conservée dans `design/references/aurelia-approved-v1.png`. La présentation est
réalisée avec des widgets GTK et des dessins Cairo, pas avec une image de fond
contenant des scores ou du texte. Aucun résultat d’illustration n’est importé.

### Cartes et comparaisons

Les cartes reprennent les fonds teintés, le score dominant, les pictogrammes
matériels, le nom du matériel entre le titre et le score et les barres glossy.
Les deux vagues ambrées du sous-menu sont des rubans vectoriels statiques :
elles ne représentent aucune mesure et n’ajoutent pas de rafraîchissement matériel.

Chaque barre représente 100 × débit B / débit A, à charge et unité compatibles,
pour une catégorie exécutée dans les deux sessions. Les sous-tests CPU sont
affichés en temps équivalent ms/cycle ; leur indice reste débit B / débit A,
ce qui équivaut à temps A / temps B. Plus haut est donc toujours mieux.
Les points CPU/GPU/RAM/stockage gardent leurs formules existantes.

L’échelle commune est au minimum 0–125, puis monte au multiple de 25 nécessaire
pour contenir le plus grand indice visible. Le trait blanc indique A=100. Le
vert indique un indice au moins égal à A, l’ambre une baisse ou une mesure
signalée variable ; ce n’est pas une preuve de significativité statistique.
Les réserves du comparateur sont conservées dans Détails et signalées par ≈
lorsqu’elles concernent les écarts. Les couleurs de fabricants restent séparées.
Sans référence compatible, aucun remplissage numérique n’est fabriqué : une
piste grisée avec un tiret indique l’indisponibilité, jamais un score zéro.

### Résultat, graphiques et actions

Le panneau Dernier résultat utilise exclusivement la date, la durée et le
protocole de la session consultée. Le badge indique une couverture CPU/GPU/RAM/
stockage terminée ou un résultat partiel. Les avertissements de variabilité
restent accessibles ; un résultat partiel ne devient pas un global complet.
La comparaison rapide présente les écarts en points et pourcentage ; son bouton
de référence ouvre le mécanisme existant de sélection A/B dans l’historique.

Le radar comporte strictement quatre axes comparables (pas de cinquième axe
décoratif). La courbe FPS présente le premier passage enregistré, avec une
surface ambrée, une origine verticale à zéro et une échelle adaptée sans couper
les pics. Le temps horizontal cumule les intervalles CPU enregistrés ; ce n’est
pas une mesure de l’affichage physique. Le débit moyen d’un segment est calculé
par nombre d’images / durée, et l’enveloppe conserve ses extrema.

La colonne de droite commence au niveau des onglets, comme sur la proposition.
Le pied de page regroupe CPU/GPU/RAM et système/noyau/pilote enregistrés, avec
les actions d’export et d’historique existantes. Le rapport textuel complet reste
consultable dans Historique. Les détails de chaque mesure restent dans Détails.
En fenêtre réduite, les quatre cartes passent par paires pour garder les scores
lisibles ; les colonnes de résultats se réorganisent sans défilement horizontal.

### Validation et limites

- 48 tests GUI réussis, deux tests ignorés par défaut (sonde I/O et capture GTK).
- Le test de capture GTK a été exécuté séparément et a réussi en 1920×1080,
  2560×1440 et 1182×800 : cinq cartes, une seule barre de vues et aucun débordement
  horizontal des cartes/panneaux détecté.
- Les captures remplies sont produites par le même rendu GTK avec un jeu de
  données synthétiques **compilé uniquement dans le binaire de test**. Elles
  sont marquées « TEST VISUEL · DONNÉES SYNTHÉTIQUES · AUCUN RÉSULTAT MESURÉ ».
  Aucun de ces résultats n’est enregistré ou exporté ; le contrôle a vérifié
  l’absence de result.json dans le dossier d’état isolé du test.
- Le binaire release ne reconnaît pas cette injection de données et ne compile
  pas `aurelia_visual_tests.rs`. Ses captures sans historique sont conservées
  séparément. Les images sont dans `design/validation/aurelia-approved-20261112/`.
- Clippy application avec `-D warnings` : sans avertissement. L’analyse étendue
  `--all-targets` relève un avertissement préexistant `items_after_test_module`
  dans `overview.rs` (fonction preview_states après le module de tests). Ce
  composant sans rapport n’a pas été remanié pour cette livraison.
- Aucun runner, protocole, constante de score, mécanisme de permission ou contrôle
  GPU n’a été modifié. Aucun monitoring supplémentaire ni dépendance ajoutée.

À vérifier sur la machine cible : un profil complet Radeon, la sélection d’une
référence compatible, l’export du résultat choisi et le déplacement entre les
écrans physiques. La VM permet la validation graphique, pas les mesures Radeon.

Fichiers concernés par cette implémentation : `aurelia_dashboard.rs`,
`aurelia_results.rs`, `workspace_nav.rs`, `system_lab.rs`, `benchmark.rs`,
`main.rs`, `overview_capture.rs`, `style.css` et le module de tests uniquement
`aurelia_visual_tests.rs` dans `crates/gameshift-gui/src/`. La référence et les
captures sont conservées dans `design/`. Aucune modification des crates moteur.

AppImage livrée : `dist/GameShift-0.5.0-alpha.2-aurelia-approved-20261112-x86_64.AppImage`.
SHA-256 et diagnostic JSON sans interface vérifiés ; stderr vide. Le binaire
release a aussi été contrôlé pour confirmer l’absence des marqueurs et du
mécanisme d’injection du jeu de données de test.

## Liseré glossy sur toutes les cartes (12 novembre 2026)

Modification ciblée de `crates/gameshift-gui/src/style.css` : contour blanc
intérieur continu de 1 px, reflet supérieur plus clair de 2 px, léger dégradé
intérieur et ombre basse sur les cartes de scores, panneaux et cartes de détails
Aurelia. Les fonds et bordures matérielles colorés sont conservés. Aucun calcul,
donnée, contrôle ou emplacement de widget modifié.

Validation : compilation incrémentale release et capture GTK avec le jeu de test
étiqueté, réussie en 1920×1080, 2560×1440 et 1182×800, sans débordement horizontal
des cartes/panneaux détecté. Images dans `design/validation/aurelia-glass-rim-20261112/`.
Aucun nouveau test unitaire ajouté pour cette modification CSS.

## Correction du reflet glossy (12 novembre 2026)

Le contour blanc uniforme ci-dessus est remplacé par des reflets directionnels
effilés, dessinés dans le composant GTK `gloss_card.rs`. Leur épaisseur et leur
opacité varient : éclat supérieur gauche, atténuation sur le haut et les côtés,
ombre basse. Les fonds et bordures colorés restent conservés. Les constructeurs
de cartes dans `aurelia_dashboard.rs` et `aurelia_results.rs` réutilisent ce
composant ; aucune mesure, formule ou donnée historique ne change.

Validation : 49 tests réussis, 2 tests interactifs ignorés ; Clippy sans
avertissement sur la cible application ; compilation release réussie. Capture
GTK dédiée réussie aux trois tailles 1920×1080, 2560×1440 et 1182×800, sans
débordement détecté. Comparaison visuelle de la capture 1080p avec la proposition
approuvée : les reflets suivent les bords des cartes et ne traversent pas leur
contenu. Captures étiquetées avec données synthétiques dans
`design/validation/aurelia-tapered-gloss-20261112-v2/`.
