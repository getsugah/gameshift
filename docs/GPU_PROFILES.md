# GameShift 0.4.0 — profils GPU au quotidien

## Démarrage

Lancer la nouvelle AppImage avec le compte normal, après avoir fermé l’ancienne
interface. Dans **GPU → Contrôle** ou **Profils**, cliquer **Activer les contrôles
GPU**. Polkit demande une autorisation administrateur pour installer les composants
fournis dans l’AppImage. Aucun terminal, téléchargement ou kit séparé n’est requis.
L’agent utilisateur démarre ensuite ; l’installation seule n’applique aucun profil.

Créer un profil depuis les éditeurs GPU : cocher les seuls paramètres souhaités,
modifier la courbe ou les valeurs, saisir « Gaming », puis **Enregistrer le profil**.
**Appliquer** enregistre et sélectionne le profil en mode manuel. Le profil actif
et la raison de sa sélection sont affichés dans **Profils**. Attendre cet état
confirmé avant de lancer une mesure comparative.

Les contrôles existants et leurs limites sont conservés : courbe PMFW, Zero RPM,
acoustique, puissance, DPM, profils de charge, fréquences et tension RDNA lorsque
le pilote expose des formats/plages reconnus. Les fréquences min et max peuvent
être sélectionnées indépendamment ; aucune borne non configurée n’est ajoutée.
La courbe PMFW utilise **Junction / Hotspot**, imposé par le firmware. Ce choix
n’est pas un sélecteur de capteur et n’est jamais substitué à la température edge.

Les réglages avancés s’autorisent dans l’éditeur, lors de la configuration du
profil. Une bascule vers un profil enregistré ne redemande ni Polkit, ni une
confirmation de 30 secondes. Modifier et enregistrer un profil déjà actif peut
entraîner sa réapplication par l’agent ; ne pas le faire pendant une mesure.

## Profils et applications

Dans **Profils**, on peut activer, modifier, renommer, dupliquer, supprimer,
choisir un profil par défaut, exporter et importer. **Modifier le GPU** recharge
les paramètres dans les éditeurs existants. Seuls les réglages explicitement
cochés sont conservés. Importer crée un nouvel identifiant, sans importer de règle
ni déclencher son activation. Le format `.gsgpuprofile` est du JSON versionné.

Pour associer une application :

1. Choisir son exécutable Linux, ou le fichier `.exe` d’un jeu Wine/Proton.
2. Pour Wine/Proton, le nom `.exe` exact est conservé ; ajouter éventuellement
   le Steam App ID pour distinguer des jeux portant le même nom.
3. Choisir le profil et sa priorité, puis enregistrer la règle.
4. Revenir en **Mode automatique** pour activer les associations.

La détection observe les processus du compte utilisateur. Une règle Linux exige
le chemin réel exact de l’exécutable. Pour Wine/Proton, un processus Wine ou `.exe`
et l’argument du jeu doivent correspondre ; Steam ou un lanceur Python seul ne
suffit pas. Les règles visant uniquement Steam sont refusées. Wine hors Steam est
pris en charge. Les scripts, conteneurs ou processus masqués/inaccessibles ne sont
pas devinés ; un exécutable enveloppe n’identifie pas forcément son application.
Les exécutables Aurelia peuvent être associés comme les autres exécutables natifs ;
les chemins internes temporaires d’une AppImage peuvent changer entre lancements.

La priorité la plus haute gagne. À égalité, l’identifiant stable de la règle
tranche, indépendamment de l’ordre des processus. L’observation a lieu environ
chaque seconde ; une sélection automatique doit rester stable 3 secondes et les
bascules automatiques sont espacées d’au moins 5 secondes. Une sélection manuelle
est traitée au prochain cycle et reste prioritaire jusqu’au retour en Automatique.
Quand une application se termine, les autres règles sont réévaluées, puis le profil
par défaut est utilisé, ou les réglages précédant la prise en charge sont restaurés.

**Pilote automatique** est une sélection manuelle sans profil : les associations
et le profil par défaut restent enregistrés. Après restauration du profil géré,
les modes ventilation PMFW/DPM sont remis en automatique lorsqu’ils sont exposés.
Les autres paramètres reviennent aux valeurs sauvegardées avant la prise en charge,
sans reset global OverDrive ni remise à zéro arbitraire du GPU. Si aucun GPU n’était
géré par l’agent, cette commande ne cible pas arbitrairement une carte.

Un profil est lié à l’identité PCI du GPU ; un import destiné à une autre carte
n’est pas appliqué ou adapté silencieusement. Les capacités et plages sont relues
avant chaque application, y compris après une reprise du pilote.

## Fenêtre, icône et CLI

L’icône est fournie par l’agent, via **StatusNotifierItem / DBusMenu**, sans GTK.
Son menu contient le profil actif, Automatique, tous les profils, Ouvrir GameShift
et Quitter l’interface / Quitter GameShift. Le clic d’ouverture et l’accès au menu dépendent du bureau.
Sous GNOME, l’extension AppIndicator doit être présente si une icône est souhaitée ;
GameShift ne l’installe pas. Sans hôte compatible, les profils et la CLI fonctionnent.

La préférence **Conserver l’icône quand la fenêtre est fermée** décide si l’icône
reste visible après fermeture. GTK quitte dans les deux cas : aucune fenêtre GTK
complète n’est maintenue en arrière-plan. **Quitter l’interface** ferme GTK et retire
l’icône ; l’agent continue. **Quitter GameShift** ferme également l’interface,
arrête proprement l’agent et utilise sa restauration habituelle des réglages GPU.
Les profils enregistrés et la préférence de démarrage de session sont conservés.
Ouvrir GameShift réactive la fenêtre existante ou relance
l’AppImage mémorisée. Si celle-ci a été déplacée, la relancer une fois depuis son
nouvel emplacement. Le nom d’application D-Bus évite les fenêtres en double.

Le démarrage avec la session est **facultatif**, activable dans Profils. Sans cette
option, ouvrir GameShift démarre l’agent s’il est installé. Fermer la fenêtre ne
stoppe pas la gestion. Il n’y a pas de réglage matériel appliqué pendant le boot
avant la session utilisateur.

Sans icône :

```sh
/usr/libexec/gameshift-profiles status
/usr/libexec/gameshift-profiles manual IDENTIFIANT_DU_PROFIL
/usr/libexec/gameshift-profiles auto
/usr/libexec/gameshift-profiles restore
/usr/libexec/gameshift-profiles retry
```

L’AppImage propose aussi `--profiles status`, `--profiles auto`, etc. Le statut
indique les identifiants, le mode demandé, le profil effectivement accepté et les
erreurs. La CLI demande un agent actif ; le bouton Démarrer l’agent ou
`systemctl --user start gameshift-profiles.service` le lance.

## Architecture et installation

Deux composants aux responsabilités séparées :

- **gameshift-profiles**, service utilisateur léger : profils personnels, règles,
  scan procfs, CLI/socket utilisateur et icône D-Bus ; aucune écriture sysfs directe.
- **gameshift-control**, service système activé par socket : identité AMDGPU,
  limites, transactions, relecture, audit, gardien thermique et restauration.
  Il reste indépendant de GTK et n’interprète aucun chemin d’écriture arbitraire.

Le premier clic installe des destinations fixes, appartenant à root : les deux
binaires dans `/usr/libexec`, les unités `gameshift-control.socket/.service`,
`gameshift-profiles.service` utilisateur, l’action Polkit existante et le compte
inscrit dans `/etc/gameshift-control/owner`. Les binaires sont préparés dans un dossier de session privé hors du montage FUSE,
puis nettoyés après l’appel. L’installateur embarqué est lui-même
exécuté avec Polkit ; l’interface reste sans privilèges. Le remplacement des fichiers
est atomique par fichier, avec tentative de retour à la version précédente en cas
d’échec d’installation. Le service système est activé seulement par son socket.

Le socket système contrôle l’UID réel du client avec `SO_PEERCRED` et accepte
uniquement le compte inscrit. Le socket utilisateur est privé dans
`$XDG_RUNTIME_DIR/gameshift/`. Les commandes sont typées : appliquer un profil,
restaurer, revenir en automatique, vérifier l’état. Le composant privilégié répète
les validations ; un fichier de profil modifié à la main n’autorise aucune écriture
hors de ces commandes. L’inscription autorise les applications de ce compte à
utiliser ces opérations matérielles, sans leur donner un accès root général.

La version et le protocole sont détectés. Aucun remplacement n’est déclenché à
chaque lancement ou mise à jour de l’AppImage ; une version compatible reste
utilisable. Le bouton de mise à jour est proposé si la version diffère, avec Polkit.
La version courante complète n’est pas réinstallée par un nouveau clic.
**Désinstaller les contrôles** arrête les composants et conserve les profils,
les historiques Aurelia et les journaux. Le démarrage automatique reste une
préférence personnelle qui peut être réutilisée à une réinstallation.

Les profils sont enregistrés dans
`$XDG_CONFIG_HOME/gameshift/gpu-profiles.json` (défaut `~/.config`), en mode 0600,
par remplacement atomique. Une configuration corrompue est signalée et conservée,
sans écrasement silencieux par un fichier vide.

## Récupération et limites

Une transaction échouée après écriture tente de restaurer les seules valeurs
concernées et signale les erreurs partielles. Une erreur n’entraîne pas une boucle
d’application incessante : le profil fautif est suspendu jusqu’à une nouvelle
sélection, une modification ou **Réessayer**. Une autre règle peut encore prendre
le relais. Les requêtes invalides sont rejetées avant toute écriture.

Le contrôleur surveille les seuils critiques hwmon disponibles, vérifie les
réglages du profil et les réapplique après une dérive, une veille ou un reset du
pilote. Une indisponibilité brève du périphérique est réessayée pendant au maximum
15 secondes ; une disparition durable, un conflit ou une erreur entraîne un retour
arrière et un état d’échec explicite. Une perte du canal de l’agent ou de son signal
de vie (15 secondes) déclenche aussi la restauration. Le redémarrage de l’agent
recharge ses profils persistants. Aucun mécanisme ne garantit une récupération
après un blocage matériel/noyau complet ou un SIGKILL du contrôleur.

Le mode interne PMFW n’est pas toujours observable ; la récupération automatique
ou la courbe initiale réapplicable restent un choix explicite. Sans seuil critique
exposé, aucun seuil thermique arbitraire n’est inventé.

L’audit et la récupération restent dans `/var/log/gameshift-control/` et
`/run/gameshift-control/`. Les journaux du service utilisateur sont consultables
avec `journalctl --user -u gameshift-profiles.service`, ceux du contrôleur avec
`journalctl -u gameshift-control.service`. La récupération manuelle existante
`pkexec /usr/libexec/gameshift-control --recover ADRESSE_PCI` reste disponible,
après arrêt de l’agent pour libérer son verrou.

Limites de cette version : systemd et une session D-Bus utilisateur sont nécessaires
pour l’intégration complète ; un seul compte inscrit et un profil actif à la fois
sur le GPU ciblé. La présence de LACT/CoreCtrl entraîne un refus conservateur pour
éviter les écritures concurrentes ; elle ne prouve pas quel GPU l’autre outil gère.
GameShift ne stoppe pas ces programmes automatiquement. Le surcoût du scan et du
gardien reste à mesurer sur le PC physique pendant Aurelia.

## Recette Fedora 44 / RX 7800 XT

La VM valide le logiciel, pas les commandes du firmware. À vérifier manuellement :

1. Fermer l’ancienne interface et les autres gestionnaires du GPU. Lancer la
   nouvelle AppImage, activer une fois les contrôles avec Polkit ; vérifier les
   permissions, versions et journaux. Ne pas désactiver SELinux en cas de refus.
2. Enregistrer deux profils standard prudents, les activer et vérifier sysfs et
   la relecture. Vérifier le retour au pilote et la restauration après erreur.
3. Associer un exécutable natif, puis un jeu Wine/Proton identifié. Vérifier les
   priorités, la fermeture du jeu, le défaut, le mode manuel et le retour Auto.
4. Fermer GTK complètement et refaire ces essais depuis l’icône et la CLI.
   Tester GNOME avec/sans AppIndicator ; aucune extension ne doit être installée.
5. Activer volontairement le démarrage avec la session, fermer/réouvrir la session
   et vérifier le profil choisi. Tester séparément veille/reprise et réapplication
   des réglages ; ne pas déclencher un reset GPU arbitraire pour le test.
6. Vérifier mise à jour, annulation Polkit et désinstallation depuis GameShift ;
   profils personnels conservés. Tester les contrôles avancés séparément, après
   décision explicite de l’utilisateur ; leurs plages ne garantissent pas la stabilité.
7. Avec un profil confirmé actif, comparer le même protocole Aurelia. Les moteurs,
   leurs scores et les formats d’historique sont conservés ; aucune causalité n’est
   déduite automatiquement d’un changement de profil.

## Références étudiées

Étude ciblée de [LACT au commit d11618a](https://github.com/ilya-zlobintsev/LACT/tree/d11618abe13f121a1298f01575d6ce5c0c9ba3b1)
: configuration et profils dans `lact-daemon/src/config.rs`, règles/priorités et
observation de processus dans `server/profiles.rs` et `server/profiles/process.rs`,
socket/permissions dans `socket.rs`, reprise logind dans `suspend.rs`.
LACT utilise notamment des événements de processus et logind ; GameShift utilise
ici procfs et la vérification des réglages du pilote, sans ajout de daemon ou
bibliothèque externe équivalente. Aucun code LACT n’a été intégré.
GIO/GLib, déjà présents dans GTK, sont réutilisés pour SNI/DBusMenu sans GTK.
