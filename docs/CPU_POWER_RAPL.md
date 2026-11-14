# Puissance CPU RAPL sur l’accueil

Le champ CPU « Puissance » affiche la **puissance moyenne du ou des packages CPU**, en watts avec une décimale. Il ne s’agit ni du TDP, ni d’une estimation fondée sur la charge ou la fréquence. Aucun graphique ou panneau n’est ajouté.

Les domaines sont découverts sous `/sys/class/powercap`, y compris les arborescences imbriquées. Seuls les noms `package-*` sont retenus ; les sous-domaines `core`, mémoire ou uncore ne sont pas additionnés au package. Les alias sont dédupliqués. Des fournisseurs distincts annonçant le même package rendent le total ambigu et donc indisponible.

Pour chaque package, deux relevés de `energy_uj` donnent :

`watts = différence d’énergie en µJ / durée monotone en µs`

Le lecteur conserve l’horodatage de chaque compteur et sa plage `max_energy_range_uj`. Le premier relevé, une erreur, une plage ou une identité de fichier modifiée, un redémarrage du helper et un intervalle hors de 100 ms–10 s donnent un état indisponible. Le débordement utilise la plage propre au domaine. Une diminution nécessitant un delta corrigé supérieur à la moitié de cette plage est traitée comme une réinitialisation ambiguë. Comme tout relevé d’un compteur cumulatif, le calcul suppose sa continuité et au plus un débordement entre deux relevés ; il ne reconstitue pas une énergie perdue pendant une longue interruption.

Les packages complets et distincts sont additionnés. Une lecture partielle n’est jamais présentée comme un total CPU. La cadence est d’environ une seconde par défaut ; un réglage d’actualisation plus lent de l’interface est respecté. La collecte utilise le worker de l’accueil et s’interrompt avec ses gardes de visibilité, benchmarks et contrôles. Les formats de résultats des benchmarks sont conservés.

## Permissions et AppImage

La lecture directe avec le compte normal est utilisée lorsqu’elle est autorisée. Si elle est refusée, l’application utilise le **helper GameShift existant**, installé de manière persistante hors du montage AppImage.

Après installation de cette version, ouvrir **GPU → Contrôle → Mettre à jour les contrôles GPU** (ou « Activer » si nécessaire). Cette opération explicite demande une autorisation Polkit. Elle met à jour les composants existants ; elle peut brièvement restaurer les réglages GPU pendant le redémarrage prévu par l’installateur. Les profils personnels sont conservés. Ne pas activer pendant une mesure ou un benchmark.

Le helper annonce `rapl_protocol: 1` dans `/usr/libexec/gameshift-control --component-version`. Un ancien helper portant le même numéro de version est désormais reconnu comme nécessitant cette mise à jour. Les installations Nix restent soumises à l’intégration déclarative déjà requise pour les composants privilégiés.

La requête fixe `RAPL1\n` sur le socket existant :

- vérifie le compte autorisé avec `SO_PEERCRED` ; le client vérifie un serveur système UID 0 ;
- accepte **aucun chemin, index de package ni commande d’écriture** ;
- lit uniquement les noms, compteurs et plages de packages sous les chemins powercap du noyau, avec limites de taille et de parcours ;
- fonctionne à côté de la session exclusive GPU, sans en prendre le verrou ni modifier son protocole JSON ;
- mutualise les demandes rapprochées dans un cache court, en conservant les horodatages d’origine.

Aucun `sudo` périodique, nouveau service généraliste ou changement de permissions sysfs n’est nécessaire. L’interface reste exécutée par l’utilisateur normal. Sans RAPL ou sans accès autorisé, elle affiche « Non disponible » ; l’infobulle et les sources en précisent la cause.

Référence : [interface powercap du noyau Linux](https://www.kernel.org/doc/html/latest/power/powercap/powercap.html).
