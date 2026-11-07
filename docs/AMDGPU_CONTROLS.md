> Version actuelle : [profils GPU 0.4.0](GPU_PROFILES.md). Installation intégrée,
> agent indépendant de GTK et changements sans confirmation de session répétée.
> Le document ci-dessous décrit le fonctionnement historique de la version 0.3.0.

# GameShift 0.3.0 — contrôles AMDGPU de test

Cette version ajoute des écritures réelles au pilote. Les tests de développement
utilisent des périphériques simulés ; **aucune écriture n’a été validée sur la
RX 7800 XT** depuis la VM Debian. L’absence de Radeon dans la VM ne préjuge pas
sa compatibilité sur Fedora. Aurelia, les historiques et `.gsbench` sont conservés.

## Périmètre livré

| Étape | Implémenté et modifiable si exposé | Limites / lecture seule |
|---|---|---|
| Registre et helper | PCI + identité AMDGPU, quatre niveaux, commandes métier Polkit, validation dans le helper | Aucun chemin arbitraire, reset, registre ou firmware modifiable |
| Refroidissement | Courbe PMFW graphique avec points déplaçables et saisie numérique, automatique, limites/cible acoustiques en RPM, PWM minimal en %, cible thermique en °C, Zero RPM et température d’arrêt | Plages et nombre de points fournis par le pilote ; pas de courbe logicielle permanente ni de backend PWM hwmon concurrent |
| Énergie | Mode DPM, profil de charge `pp_power_profile_mode`, limite de puissance hwmon en W dans l’interface | Les profils de charge viennent du pilote. Les huit noms DPM documentés sont des candidats, pas une liste garantie par le matériel ; un refus reste explicite |
| Fréquences / tension | Min/max GPU et VRAM RDNA et décalage de tension en mV, selon le format et `OD_RANGE` | Confirmation avancée ; anciennes tables GCN, courbes tension/fréquence et masques `pp_dpm_*` non implémentés |
| Laboratoire / GPU | Surveillance, Contrôle et Exploration séparés ; catégories fonctionnelles, filtres et détails bruts | Interfaces inconnues et debugfs non validées : contenu non ouvert automatiquement ; commandes critiques interdites |
| Aurelia | Relevés des réglages avant/après l’évaluation système et avertissement s’ils changent ; rapports individuels/comparaison | Pas de nouvelle formule de score, protocole ou causalité déduite ; les autres moteurs conservent leurs formats |

Les contrôles sont proposés seulement lorsque les formats et limites sont
reconnus. Les bits d’écriture sysfs ne remplacent ni l’autorisation Polkit, ni la
validation côté helper. Les refus du pilote, d’OverDrive ou des permissions sont
rapportés. Les états « non lu », « lecture refusée », « indisponible », « non pris
en charge », « écriture non implémentée » et « écriture autorisée » sont distincts.
Les attributs d’un même capteur hwmon sont regroupés dans le Laboratoire.

Le seuil `fan_zero_rpm_stop_temperature` est une **température d’arrêt** ; il ne
faut pas le présenter comme un seuil de reprise garanti. Le PWM PMFW est en
pourcentage, contrairement à certains attributs hwmon en 0–255. La température
des points de courbe PMFW est celle indiquée par le pilote (hotspot sur le format
pris en charge), distincte d’une température GPU edge.

## Installer le helper sur Fedora

Décompresser le kit `GameShift-control-helper-0.3.0.tar.gz`, puis :

```sh
cd GameShift-control-helper-0.3.0
sha256sum -c SHA256SUMS
sudo ./install-helper.sh
```

L’installation est **manuelle**. Le script installe seulement :

- `/usr/libexec/gameshift-control`, root:root, 0755 ;
- `/usr/share/polkit-1/actions/io.github.gameshift.control.policy`, 0644.

Aucun service, groupe privilégié, réglage de démarrage ou paramètre noyau n’est
ajouté. L’action Polkit demande une authentification administrateur pour chaque
session active ; elle ne conserve pas l’autorisation. Un agent Polkit de session
est nécessaire. Le helper est autonome, sans GTK ni dépendance ROCm.
GameShift vérifie sa version (`0.3.0`, protocole 1) et ses permissions.

Lancer ensuite l’AppImage **avec le compte normal**, puis GPU → Contrôle.
Le helper ne doit pas être lancé depuis un répertoire utilisateur par `pkexec`.
Le kit contient une copie du guide ; l’AppImage contient également le binaire
et l’installateur sous `usr/bin` et `usr/share/gameshift-control` après extraction.

## Fonctionnement d’une session

1. Choisir le GPU PCI, relire les valeurs et cocher uniquement les changements.
2. Ajuster les valeurs dans les limites affichées ; les valeurs des éditeurs
   restent les relevés **avant** application. Le reçu affiche les valeurs relues.
3. Pour les fréquences, le décalage de tension ou les modes de profilage DPM,
   cocher l’autorisation avancée. Examiner le récapitulatif, puis Appliquer.
4. Autoriser Polkit. Le helper relit l’identité, les capacités et l’état courant,
   enregistre l’état initial, applique les commandes, puis vérifie la relecture.
5. Confirmer sous 30 secondes ou laisser restaurer. Après confirmation, une
   session dure au maximum une heure. Aurelia est accessible après confirmation.
6. Restaurer termine la session. Pour essayer un autre ensemble, restaurer,
   relire, modifier le brouillon et réappliquer. Une seule transaction active
   par GPU ; pas d’empilement de réglages ni de persistance au démarrage.

Une courbe personnalisée et les réglages acoustiques/thermiques qui sélectionnent
le mode automatique sont incompatibles dans une transaction : le helper refuse
leur combinaison. Zero RPM peut être associé à une courbe. Les plages sont celles
du pilote, sans bornes matérielles inventées. Un brouillon de courbe interpolé
peut être affiché quand le relevé automatique contient des zéros ; il n’est jamais
appliqué sans sélection et confirmation.

Le mode interne PMFW n’est pas toujours lisible : GameShift ne prétend pas le
reconstruire. La récupération choisie est explicite : **automatique** par défaut,
ou **courbe initiale lue**, seulement si ses points sont réapplicables. La commande
`r` de `fan_curve` restaure le comportement automatique du firmware ; ce n’est pas
une réinitialisation matérielle du GPU. Les commandes globales de reset OverDrive
ne sont pas utilisées pour restaurer des réglages sans rapport.

Les modes DPM et profils de charge sont distincts. Le helper active le mode
manuel lorsque la commande l’exige, vérifie le résultat et restaure le mode
initial en dernier. Une valeur arrondie ou refusée par le pilote est une erreur
explicite suivie d’une tentative de retour arrière, pas un succès silencieux.

## Récupération et limites

Un processus privilégié temporaire reste vivant pendant la session. Il restaure
sur fermeture du canal GTK, délai, signal INT/TERM/HUP ou erreur de traitement.
Il compare aussi les températures aux seuils critiques hwmon réellement exposés,
environ chaque seconde ; dépassement ou perte d’un capteur précédemment disponible
entraîne une restauration. Aucun seuil arbitraire n’est substitué aux seuils
absents. Sans seuil disponible, seule la protection temporelle est assurée par
GameShift ; la régulation propre au pilote reste déterminante.

L’audit privilégié est dans `/var/log/gameshift-control/`, la récupération
root uniquement dans `/run/gameshift-control/`. Une trace utilisateur est dans
`$XDG_STATE_HOME/gameshift/hardware-actions/` (défaut `~/.local/state`).
Le journal est écrit avant les changements. Une erreur partielle de restauration
reste visible et bloque une nouvelle session jusqu’à récupération explicite :

```sh
pkexec /usr/libexec/gameshift-control --recover 0000:03:00.0
```

Remplacer l’adresse par celle effectivement détectée. La commande vérifie
l’identité du GPU et utilise uniquement l’état enregistré par le helper.

Un SIGKILL du helper, un blocage noyau, une suspension prolongée ou un blocage
matériel complet ne garantissent aucune récupération immédiate. Les fichiers de
récupération de `/run` ne survivent pas au redémarrage. Aucun retour arrière ne
peut être promis après un blocage matériel complet. Ne pas faire piloter le même
GPU simultanément par plusieurs outils ; la détection de changement initial et
le verrou GameShift n’excluent pas les écritures ultérieures d’un autre programme.

## Recette progressive sur Fedora 44 / RX 7800 XT

Tests interactifs à effectuer par l’utilisateur ; aucune commande de réglage
n’est lancée automatiquement par la procédure de construction.

1. Garder l’ancienne AppImage. Arrêter les autres gestionnaires du GPU avant
   l’essai, puis installer le kit vérifié. Ne pas changer les paramètres noyau.
2. Lancer `--diagnose-json`, `--driver-lab-json` et `--controls-json` pour conserver
   les relevés en lecture seule. Vérifier PCI, pilote, unités et limites contre
   les fichiers du périphérique concerné ; aucun indice card/hwmon présumé.
3. Dans GPU, vérifier Surveillance / Contrôle / Exploration, filtres, détails,
   tailles étroite et large et lisibilité. Aucun réglage coché ne s’applique seul.
4. Pour un premier essai standard, si Zero RPM est actif et proposé, sélectionner
   sa désactivation, autoriser Polkit et confirmer. Vérifier le reçu et sysfs.
   Restaurer et vérifier le retour de la valeur initiale. Si ce réglage n’existe
   pas, choisir explicitement un autre réglage standard adapté au matériel.
5. Répéter en laissant expirer les 30 secondes, puis en fermant GameShift après
   confirmation. Vérifier la restauration et les journaux indépendants de GTK.
6. Refuser Polkit, tester une sélection incompatible courbe/acoustique et annuler
   une confirmation. Les valeurs hors limites sont testées par les tests simulés,
   sans contourner volontairement les protections du pilote sur le matériel.
7. Valider séparément les contrôles acoustiques, Zero RPM, automatique, puis une
   courbe prudente vérifiée. Comparer les points retournés et la récupération
   choisie ; ne pas déduire le mode interne d’une simple valeur de courbe.
8. Modes DPM, profils de charge et puissance : petites modifications explicites,
   relecture et restauration après chaque essai. Les modes DPM rejetés doivent
   afficher un refus, sans prétendre que tous sont supportés.
9. **Seulement après décision explicite de l’utilisateur**, valider séparément
   fréquences et tension. L’exposition d’une plage ne garantit pas la stabilité.
10. Après confirmation d’un profil, lancer deux fois le même protocole Aurelia,
    restaurer, essayer un autre profil et comparer. Examiner réglages avant/après,
    fréquences, températures et puissances disponibles. Distinguer mesures et
    hypothèses ; une variation ne prouve pas sa cause.

## Validation logicielle et références

Tests en mémoire : parsing des plages/unités, validation intégrale avant écriture,
refus des chemins/champs inconnus, commits PMFW, relecture, modes incompatibles,
identité et modifications concurrentes, fréquences ordonnées, tension/puissance,
restauration partielle et seuils thermiques simulés. Les tests ne valident ni les
permissions réelles de Fedora, ni les commandes du firmware, ni l’interface GTK.

Étude technique sans intégration de code externe :

- [LACT](https://github.com/ilya-zlobintsev/LACT/tree/d11618abe13f121a1298f01575d6ce5c0c9ba3b1), commit étudié `d11618a`, licence MIT ;
- [amdgpu-sysfs-rs](https://github.com/ilya-zlobintsev/amdgpu-sysfs-rs/tree/da58cda74a514a259f81b7b42e8652c58c22b33b), commit `da58cda` ;
- [Interfaces thermiques / DPM / OverDrive du noyau](https://docs.kernel.org/gpu/amdgpu/thermal.html) ;
- [Implémentation SMU 13.0.7](https://github.com/torvalds/linux/blob/master/drivers/gpu/drm/amd/pm/swsmu/smu13/smu_v13_0_7_ppt.c), notamment commandes PMFW `c`/`r` et interactions des modes ;
- [Documentation pkexec](https://www.freedesktop.org/software/polkit/docs/latest/pkexec.1.html).

Aucune dépendance LACT, ROCm, daemon permanent ou nouvelle bibliothèque externe
n’est ajoutée. La crate interne `gameshift-control` partage les parseurs entre
registre, GUI, helper et relevés Aurelia.
