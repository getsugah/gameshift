# Journal des changements

## 2026-11-07 — Alpha 0.5.0-alpha.2

- Menu de notification : ajout de « Quitter GameShift » pour fermer l’interface et arrêter proprement l’agent, avec restauration habituelle des réglages et conservation des profils.
- Présentation publique sans référence à une communauté ou distribution particulière ; documentation et titre du diagnostic harmonisés.
- Numéro de version incrémenté pour permettre la mise à jour des agents déjà installés.

## 2026-11-07 — Alpha communautaire 0.5.0-alpha.1


- Aurelia : accueil par domaines, matériel et dernier résultat ; lancement, suivi, résultats et historique séparés, cartes de mesures et détails repliés. Moteurs, charges, scène Vulkan, protocoles et .gsbench préservés ; validations Vulkan facultatives, aucun score ajouté.
- Processus GPU : DRM fdinfo sans helper, inventaire PCI AMD/Intel/NVIDIA, tableau triable, VRAM/GTT attribuées, temps et taux par moteur, historique borné, gestion des permissions/disparitions et suspension pendant Aurelia. Compteurs cycles seuls et backend NVIDIA privé non implémentés. Codecs VA-API sur requête explicite si vainfo est déjà présent.
- Profils conservés ; barrière utilisateur et protocole contrôleur 3 pour différer les bascules/réapplications automatiques pendant les mesures. Métadonnées de profil avant/après système et Vulkan, sans changement des calculs.
- Diagnostic avec aperçu et sélection de résultats, filtrage par liste blanche, aucun envoi automatique ni export brut des processus/identités personnelles.
- Sources publiques sélectionnées avec manifeste et détection de motifs privés ; licence GPL déjà déclarée explicitée. Recettes sans chemin vers un autre projet ; contraintes Nix/FHS documentées et bootstrap impératif désactivé sur NixOS. Aucune installation système ni publication effectuée pendant le développement.
- Validation VM : 128 tests réussis, Clippy tous targets sans avertissement, formatage et huit sources historiques Vulkan vérifiés ; recette GTK et matérielle Fedora/Radeon, Intel/NVIDIA et systèmes basés sur Nix restant à effectuer.

## 2026-11-07 — Profils GPU persistants 0.4.0 (recette Fedora requise)

- Activation intégrée, version et désinstallation via Polkit ; destinations système fixes, compte inscrit, unités socket/service et tentative de rollback d’installation. Aucun téléchargement ni réglage noyau/BIOS.
- Agent utilisateur sans GTK : profils versionnés et atomiques, CRUD/import/export, mode manuel persistant, défaut, règles natives/Wine/Proton, priorités déterministes, stabilisation des changements ; Steam seul ne déclenche pas un profil.
- Contrôleur système à la demande : transactions AMDGPU existantes réutilisées, validation privilégiée, relecture, restauration, surveillance thermique et réapplication après reprise/dérive. Aucun chemin sysfs/debugfs arbitraire.
- Section Profils et éditeurs GPU partagés ; ventilation explicitement Junction/Hotspot, bornes de fréquence indépendantes. Icône SNI/DBusMenu dynamique et CLI ; GTK quitte à la fermeture, agent autonome ; démarrage avec la session facultatif.
- Moteurs Aurelia, monitoring, scores et formats d’historique conservés. GIO déjà utilisé par GTK devient une dépendance directe de l’agent, sans nouvelle bibliothèque externe installée.
- Validation : 120 tests uniques réussis (dont D-Bus isolé), 4 épreuves matérielles/Vulkan laissées en opt-in ; clippy sans avertissement, formatage et syntaxe des installateurs/unités vérifiés. Huit sources historiques Aurelia v3 inchangées.
- Tests unitaires des profils/règles/IPC et transactions simulées, test D-Bus isolé sans GTK ni écriture GPU ; installation système, GNOME et RX 7800 XT restent à valider sur Fedora. Guide et limites : docs/GPU_PROFILES.md.


## 2026-11-07 — Contrôles AMDGPU 0.3.0 (recette matérielle requise)

- Crate interne gameshift-control : parseurs PMFW/RDNA, validation métier, identité PCI/AMDGPU et écritures sysfs explicitement autorisées. Aucun paquet externe supplémentaire.
- Helper Polkit à la demande : audit avant écriture, état initial, commits, relecture, rollback partiel, confirmation 30 s, session limitée à une heure, restauration indépendante de GTK et surveillance des seuils thermiques exposés. Kit d’installation séparé, aucun service ni réglage de démarrage.
- Contrôles réels selon interfaces et plages détectées : ventilation PMFW (courbe ajustable, acoustique, PWM minimal, température cible et Zero RPM), modes DPM, profils de charge, puissance, fréquences RDNA et décalage de tension avec confirmation avancée.
- GPU : espaces Surveillance / Contrôle / Exploration ; laboratoire fonctionnel, filtres et quatre niveaux, états de lecture/écriture explicites, attributs hwmon regroupés. Réglages initiaux et reçus distingués.
- Aurelia système : relevés des réglages avant/après dans les paramètres existants, avertissement en cas de changement et rapports consultables ; protocoles, scores et formats d’historique conservés.
- Limites : pas de masque DPM modifiable, anciennes tables OverDrive et backend PWM logiciel non implémentés ; mode interne PMFW non observable, récupération automatique ou courbe initiale explicitement choisie. Aucun reset GPU, écriture debugfs/registre ou opération PCI destructive.
- Validation logicielle : 103 tests réussis et 4 tests matériels/Vulkan opt-in ignorés (comptage sans double exécution), clippy workspace tous targets sans avertissement, rustfmt/diff, recettes Python/shell/XML et huit sources historiques Aurelia v3 vérifiés. Construction incrémentale AppImage Debian ; aucune commande matérielle ni automatisation graphique lancée. Installation/recette Fedora détaillées dans docs/AMDGPU_CONTROLS.md. Aucune validation RX 7800 XT revendiquée.


## 2026-11-06 — Navigation du centre matériel

- Rubriques Accueil, GPU, CPU, Benchmarks et Paramètres ; vues existantes conservées.
- Fiches Système/Mémoire/Stockage accessibles depuis les cartes de l’Accueil.
- Sélecteur de thème déplacé dans Paramètres, sans persistance ajoutée.
- Conservation de la page sélectionnée et des widgets Benchmarks/Paramètres lors de l’actualisation.
- Collecteurs, moteurs Aurelia, formats et dépendances inchangés.
- Validation : cargo check -p gameshift-gui --offline --locked réussi dans la VM Debian ; git diff --check et rustfmt ciblé réussis.
- Recette manuelle Fedora : parcourir les cinq rubriques à la souris ; ouvrir chaque fiche depuis l’Accueil ; actualiser depuis une fiche, Benchmarks et Paramètres ; vérifier la sélection et le thème conservés. Vérifier aussi les modes clair/sombre et une fenêtre étroite. Aucun test graphique automatisé lancé.
- Aucun réglage matériel appliqué. La VM ne valide pas les capacités Radeon.

## 2026-11-06 — Socle de télémétrie AMDGPU

- Découverte liée à l’adresse PCI sélectionnée et au pilote amdgpu ; énumération des hwmon amdgpu et de leurs entrées, sans indice cardX/hwmonY fixé.
- Mesures structurées : type, label, unité, source, horodatage Unix en millisecondes, valeur optionnelle et erreur. Disponibilité définie par la présence d’une valeur ; les erreurs restent explicites.
- Températures nommées, fréquences GPU/VRAM identifiées par label ou état DPM actif, puissance, ventilateurs, occupation et VRAM selon les fichiers exposés. Aucun gpu_metrics ni contrôle matériel ajouté.
- Séries intégrées au collecteur Aurelia existant ; anciens champs conservés et nouveaux champs optionnels à la lecture des anciens rapports. La température historique n’est renseignée que pour un capteur explicitement nommé edge ; aucun hotspot n’est substitué.
- Cadence réglable par API entre 200 ms et 60 s ; défaut de 500 ms conservé pour Aurelia. Pas encore de réglage dans la GUI ni de surveillance continue des pages matérielles. Le surcoût réel reste à mesurer sur Fedora.
- Validation VM : 6 tests réussis, 1 test matériel ignoré ; cargo check GUI et clippy télémétrie tous targets avec avertissements interdits réussis ; aucune nouvelle dépendance.
- Recette Fedora : lancer une évaluation système depuis Benchmarks, consulter le JSON conservé dans l’historique, vérifier gpu_sensors et les measurements de ses samples contre les fichiers sysfs du même périphérique PCI. Vérifier unités, labels edge/junction, sources et erreurs ; ne pas supposer la présence de chaque capteur. Comparer plusieurs passages avec/sans collecte avant de revendiquer un surcoût négligeable.
- Suite du jalon : intégration visuelle continue et télémétrie CPU ; aucun score ni protocole de charge modifié ici.

## 2026-11-06 — Affichage CPU en direct

- Accueil : occupation globale CPU ; page CPU : occupation globale et détail dépliable des processeurs logiques, fréquences observées et températures disponibles.
- Réutilisation de gameshift-cpu::environment et de ses capteurs CPU hwmon dynamiques ; calcul d’occupation extrait en fonction commune, sans changement de formule du benchmark.
- Relevés hors du thread GTK, environ une fois par seconde, une seule demande en cours par panneau. Aucune nouvelle demande lorsque la page est masquée ; une lecture déjà engagée peut se terminer puis son résultat est écarté. Fermeture/destruction : timer libéré par références faibles et worker ponctuel terminé après lecture.
- Provenance et horodatage consultables ; absence de fréquence/température explicite. Première occupation indisponible jusqu’au deuxième relevé. Aucun stockage continu, graphique, mesure énergétique ou réglage CPU ajouté.
- Tests : 7 tests unitaires gameshift-cpu réussis ; cargo check GUI et clippy CPU/GUI avec avertissements interdits réussis ; diff et formatage ciblés vérifiés.
- Recette manuelle Fedora : ouvrir Accueil puis CPU ; attendre deux relevés ; déplier les processeurs logiques ; vérifier unités, fréquences et températures selon disponibilité ; passer dans Benchmarks puis revenir sur CPU ; actualiser le diagnostic, fermer et rouvrir la fenêtre. Vérifier l’absence de blocage et le nouveau départ de mesure, puis le comportement en fenêtre étroite. Aucune recette graphique automatisée ni validation Radeon dans la VM.
- Reste du jalon télémétrie : affichage GPU continu, cadence dans Paramètres, séries temporelles et puissance CPU lorsque disponible.

## 2026-11-06 — GPU en direct et cadence de session

- Accueil : synthèse GPU AMD ; page GPU : mesures disponibles par périphérique PCI (températures nommées, fréquences, puissance, ventilateurs, occupation, VRAM). Sources, horodatages et erreurs dans un volet dépliable. Absence d’amdgpu explicitée, sans substitution par le GPU virtuel.
- Énumération dynamique de tous les périphériques liés au pilote amdgpu, testée sur deux GPU et un pilote virtuel exclus. Aucun index cardX/hwmonY fixé ; aucune capacité déduite de la VM.
- Lectures GPU hors GTK, une demande en cours par panneau ; suspension hors Accueil/GPU, résultat masqué écarté. Redécouverte à chaque relevé pour tenir compte des changements de périphériques/capteurs.
- Paramètres : intervalle de session 500 ms / 1 s / 2 s / 5 s partagé par les vues CPU et GPU, conservé lors de l’actualisation du diagnostic. Les collecteurs internes Aurelia gardent leurs propres cadences. Aucun stockage de préférence ni courbe ajoutés.
- Dépendance GUI vers la crate de télémétrie déjà présente dans le workspace ; aucun nouveau paquet ni mise à jour des versions verrouillées.
- Validation : 7 tests télémétrie réussis, 1 test matériel ignoré ; cargo check GUI et clippy GUI/télémétrie tous targets avec avertissements interdits réussis ; diff/formatage ciblés vérifiés. Aucun benchmark ni test graphique exécuté.
- Recette Fedora : ouvrir Accueil puis GPU, vérifier les adresses PCI, unités et labels contre sysfs ; vérifier les absences et sources ; changer la cadence dans Paramètres, revenir sur CPU/GPU puis actualiser le diagnostic ; ouvrir Benchmarks pour vérifier la suspension des vues. Tester fermeture/réouverture et fenêtre étroite. Mesurer le surcoût réel avant toute revendication de neutralité.
- L’AppImage existante n’a pas été reconstruite. Restent séries temporelles/courbes, énergie CPU et persistance des préférences.

## 2026-11-06 — Courbes d’occupation CPU/GPU

- Courbe CPU sur Accueil/CPU et courbe distincte par GPU PCI sur Accueil/GPU ; échelles identiques de 0 à 100 %, fenêtre de 60 s. Courbes et relevés textuels coexistent ; aucun mélange de températures, fréquences et occupation sur un même axe.
- Historique uniquement en mémoire, maximum 121 points par série et rétention de 60 s. Axe temporel fondé sur le temps monotone de réception des relevés, pas sur leur numéro. Modification de cadence sans modification d’échelle.
- Valeurs absentes/invalides représentées par une interruption, jamais par zéro ; points isolés visibles. Pause des collecteurs marquée par une interruption. Absence de mesure explicitée sur la courbe.
- Dessin GTK/Cairo existant, sans dépendance supplémentaire ni timer d’animation/lecture additionnelle. Séries indépendantes par panneau ; actualiser le diagnostic reconstruit les panneaux et efface ces séries temporaires. Disparition GPU : sa courbe est retirée.
- Validation : 9 tests télémétrie réussis, 1 test matériel ignoré ; cargo check GUI et clippy GUI/télémétrie tous targets avec avertissements interdits réussis. Formatage et diff vérifiés. Aucun benchmark ni test graphique lancé.
- Recette Fedora : attendre plusieurs relevés CPU/GPU, vérifier les axes et les points ; modifier la cadence ; quitter/revenir sur la page et vérifier la coupure ; actualiser le diagnostic et vérifier la remise à zéro de l’historique temporaire. Vérifier les modes clair/sombre, fenêtre étroite et indisponibilités des capteurs. La VM ne valide pas la Radeon.
- Pas de persistance/export des séries de surveillance ni de courbes température/puissance ajoutés ici. Les séries de benchmark et leurs formats demeurent inchangés. AppImage non reconstruite.

## 2026-11-06 — GameShift 0.2.0, livraison de surveillance/Aurelia

- Préférences versionnées et persistantes, thème sombre par défaut, écriture atomique ; fichiers futurs/invalides conservés avec erreur visible.
- Historique système : recherche, export natif .gsbench (JSON versionné existant), import byte-identique, sélection du nouveau résultat, référence conservée et retrait réversible dans removed. Historiques CPU/raster existants conservés séparément.
- Comparateur système : mode inter-machines explicite, même AppImage requise, paramètres conservés ; adaptation uniquement de l’identité GPU et du nombre de workers multithread prévu par le protocole. Les tailles RAM ou qualités différentes restent incompatibles. Comparaison stricte historique conservée par défaut.
- Écarts absolus/pourcentages, barres à échelle commune, version des indices et contributions additives en points de synthèse issues de la décomposition logarithmique des poids existants. Aucune nouvelle pondération. Faits et hypothèses séparés.
- Packaging adapté au Debian migré, hors ligne et sans Podman ; compilation de trois binaires avec deux jobs, sysroot et caches réutilisés. Anciennes images et anciens répertoires d’assemblage conservés ; manifeste Git/SHA-256 embarqué.
- Validation source : 82 tests uniques réussis, 4 tests matériels/Vulkan opt-in ignorés. Premier passage GUI bloqué par libadwaita absent du chemin système ; relance ciblée réussie avec le runtime existant du sysroot. Clippy workspace/tous targets, formatage et vérification v3 réussis. Aucun test graphique automatisé.
- La livraison reste en lecture seule pour le matériel : contrôle privilégié et profils matériels, nouveaux domaines stockage/compute et superposition des courbes de benchmark non implémentés. ROCm/HIP/PDF restent facultatifs et différés. Le cahier des charges initial complet n’est pas déclaré achevé.
- Guide de recette et périmètre embarqués : docs/RECETTE_0.2.md. Compilation AppImage et validation du paquet consignées après construction.

### Isolation finale des benchmarks

- Garde partagée entre fenêtres et laboratoires : les relevés en direct restent suspendus pendant tout traitement du laboratoire, même si l’utilisateur change de page.
- L’actualisation du diagnostic est différée lorsque le laboratoire est actif, y compris si le test démarre après le début d’une collecte, pour éviter le détachement des widgets et l’annulation du test.
- Test de garde multi-fenêtres et de libération des pages ajouté. La première image d’assemblage est conservée dans dist/pre-release ; le livrable principal est reconstruit avec cette correction.

### Validation du livrable 0.2.0

- AppImage construite hors ligne : dist/GameShift-0.2.0-x86_64.AppImage, 28027384 octets. Ancienne image et premier assemblage conservés.
- SHA-256 : 2f16ccffaee7c04894e32342eef2f78a2f902b389e23805e72cff63a3d6c73e3.
- Sources embarquées : f9599d036a38bbe9503edf7a7f6f0852fd5aef59 ; arbre propre lors de la construction. Empreintes des trois exécutables et des 95 bibliothèques extraits de l’image vérifiées.
- Audit des 98 ELF réussi, plafond glibc 2.41. Diagnostic sans fenêtre, plan système v2 et smoke test glibc réussis depuis l’AppImage avec extraction sans FUSE ; aucun chemin de bibliothèques du SDK injecté dans ces lancements.
- Rapports détaillés : target/release-validation/0.2.0-i4plat7k. Guide et synthèse copiés dans dist à côté de l’AppImage.
- Recette GTK/Radeon physique à effectuer sur Fedora ; aucune validation visuelle ou fonction de contrôle matériel revendiquée. Ce commit consigne les résultats après construction sans modifier les exécutables du livrable.

## 2026-11-06 — Prototype GPU et exploration sûre AMDGPU

- Refonte limitée à GPU : identité graphite/turquoise, cartes compactes, courbe agrandie, sélection PCI et accès à Aurelia. Données des collecteurs existants, diagnostic initial toujours disponible.
- Laboratoire manuel Observation/Contrôle/Expérimental, recherche, provenance, présence, erreurs, permissions non assimilées à une écriture sûre. Plages exposées conservées, limites et absence d’adaptateur explicités.
- Inventaire borné et liste explicite de lectures ; interfaces inconnues ou binaires en métadonnées seules. Reset, éviction, tests/debug à effet de bord et accès bas niveau jamais ouverts. Aucune écriture matérielle, opération Xorg/compositeur ou élévation de privilèges ajoutée.
- Isolation de l’inventaire et des graphes par GPU ; effacement à la sélection. Une erreur de pilote sur un autre périphérique ne masque plus les GPU lisibles. debugfs facultatif et dédupliqué.
- Diagnostic CLI --driver-lab-json, sans fenêtre et avec rejet des chemins arbitraires. Pas de changement aux protocoles, historiques, scores ni exports Aurelia.
- Validation ciblée : 13 tests télémétrie, 12 tests unitaires GUI et 3 tests de diagnostic réussis ; 4 tests matériels/Vulkan opt-in ignorés. Clippy GUI/télémétrie tous targets avec avertissements interdits, formatage et vérification des sources v3 réussis.
- Recette visuelle et matérielle requise sur Fedora ; aucune automatisation GUI exécutée. Généralisation du style et contrôles Polkit différés jusqu’à validation de la page représentative. Guide : docs/GPU_DESIGN_PILOT.md.

### Livrable de prévisualisation GPU

- dist/GameShift-GPU-preview-x86_64.AppImage : 28097016 octets ; SHA-256 06a47a9fa926d34b15713d2a67149288989305894cb557e22e43b11535ed49f1.
- Sources embarquées : d565a2981681ef88b0d18607468c71476179a022, arbre propre lors de l’assemblage. Intégrité des trois exécutables et 95 bibliothèques et audit glibc <= 2.41 vérifiés depuis l’image extraite.
- Diagnostic sans fenêtre, inventaire --driver-lab-json en lecture seule et plan Aurelia système v2 réussis depuis l’AppImage sans injection du runtime SDK. Anciennes images conservées, aucune publication.
- Guide de recette et synthèse de validation copiés dans dist. Rapports : target/release-validation/gpu-preview-y15ebu4p. La validation visuelle et Radeon physique reste manuelle ; aucune généralisation aux autres pages avant retour utilisateur.
