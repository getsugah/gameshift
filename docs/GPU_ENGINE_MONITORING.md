# Activité des moteurs GPU sur l’accueil

Le panneau remplace les anciennes tuiles RAM, VRAM, stockage et puissance. La mémoire et la puissance déjà présentées dans la carte GPU restent disponibles. Aucun protocole Aurelia, profil, contrôle matériel ou format de fichier sauvegardé n’est modifié.

## Lecture des courbes

- **3D / GFX, Compute, Encodage vidéo** : différences des compteurs `drm-engine-*` en nanosecondes, divisées par le temps monotone écoulé, affichées en **ms/s cumulées de clients DRM observables**. Il ne s’agit pas d’un pourcentage d’occupation matérielle. Les clients et instances de moteurs peuvent travailler simultanément : une valeur supérieure à 1 000 ms/s est possible et n’est pas plafonnée. L’axe vertical s’ajuste aux valeurs effectivement relevées.
- **Vidéo / VCN** : pourcentage matériel fourni par `vcn_busy_percent`, validé entre 0 et 100. VCN peut inclure l’encodage ; ce graphique ne représente pas un moteur de décodage indépendant.
- L’historique couvre environ 60 secondes. Les valeurs absentes, premiers relevés et régressions produisent des interruptions, jamais des zéros artificiels. Un compteur accessible et constant donne réellement 0 ms/s.
- Les sources PCI, noms exacts et limites de couverture figurent dans les infobulles. Les permissions de `/proc`, associations ambiguës et limites du parcours peuvent rendre la collecte partielle. Cette mesure ne couvre pas nécessairement tous les processus ni les activités internes du GPU.

Les familles gfx/graphics/3d, compute et enc/encode, avec suffixes d’instances numériques, sont reconnues. Les autres noms restent disponibles dans le collecteur générique ; ils ne sont pas arbitrairement assimilés à un moteur connu. Aucun compteur DEC n’est supposé présent.

## Collecte et machines multiples

La découverte part de `/sys/class/drm/card*`, résout le périphérique PCI et vérifie le pilote `amdgpu`. Aucun numéro de carte ni adresse PCI n’est fixé. Avec plusieurs Radeon, le sélecteur du panneau choisit également le GPU de la carte matérielle et du top des processus de l’accueil. Les historiques sont effacés lors du changement de GPU.

Le gestionnaire de processus et l’accueil utilisent le même tracker et un instantané partagé, depuis leurs workers. Aucun parcours de `/proc` ne s’effectue sur le thread GTK ni à chaque dessin. Le cache suit la cadence demandée, bornée entre 500 ms et 5 s ; un instantané réutilisé ne crée pas un nouveau point DRM. La liste des processus et les nouvelles courbes proviennent du même relevé.

Les clients partagés sont dédupliqués par périphérique, pilote et identifiant DRM ; les descripteurs ne sont pas additionnés. Les propriétaires sont identifiés par PID et temps de démarrage. Nouveaux clients et identifiants réutilisés nécessitent une référence initiale. Les clients disparus sont retirés. Une régression conserve la valeur haute précédente et suspend la mesure jusqu’au rattrapage, conformément au comportement documenté des compteurs DRM. Cela évite un pic artificiel au retour du compteur ; une réinitialisation persistante reste indisponible plutôt que de deviner une activité.

La collecte est en lecture seule, sans privilège supplémentaire. Elle suit la visibilité de l’accueil et les gardes existantes des benchmarks et contrôles matériels. Un consommateur suspendu n’arrête pas le tracker des autres consommateurs. Une longue période sans consommateur réinitialise les références.

Sans mesure accessible, le panneau reste compact et indique « Non disponible ». Les moteurs absents sont listés sans grandes courbes vides. Les prises en charge Intel et NVIDIA existantes du gestionnaire de processus restent inchangées ; aucun backend avancé n’est ajouté ici.

Spécification des compteurs : [Linux DRM client usage stats](https://www.kernel.org/doc/html/latest/gpu/drm-usage-stats.html).

## Validation

Les tests utilisent des arborescences temporaires indépendantes : découverte DRM et deux GPU, validation sysfs, compteurs absents ou invalides, déduplication, taux non plafonnés, premiers relevés, clients nouveaux ou terminés, réutilisation de PID, régressions, pauses et valeurs constantes. Les données de test ne sont jamais injectées dans les captures de l’accueil réel.

Sur la VM de développement, les captures vérifient l’état sans GPU AMD compatible en 1182 × 752 et 2560 × 1440. Une validation physique des quatre courbes reste nécessaire sur une Radeon exposant ces compteurs, avec une charge graphique, compute puis d’encodage choisie par l’utilisateur. Aucun benchmark n’est lancé automatiquement pour cette validation.


## Vérification des pourcentages et unités

Les quatre emplacements conservent leur disposition adaptative, leurs couleurs, leurs dimensions et leur historique temporel de 60 secondes. Aucun autre panneau, backend de contrôle, benchmark ou script de packaging n’est modifié par cette passe.

| Moteur | Source exacte, pour le PCI sélectionné | Formule affichée | Nature et limite |
|---|---|---|---|
| GFX / 3D | `/proc/<pid>/fdinfo/<fd>` : `drm-engine-gfx` et alias reconnus | `Σ Δtemps_client_ns / (Δtemps_monotone_s × 10⁶)` en ms/s | Temps des clients observables. Pas de conversion en pourcentage matériel fiable. |
| Compute | Même relevé : `drm-engine-compute` et instances reconnues | Même formule en ms/s | Le calcul peut s’exécuter simultanément sur plusieurs clients/instances. |
| Encode | Même relevé : `drm-engine-enc` / `encode` et instances reconnues | Même formule en ms/s | Un cumul n’est pas une union des périodes d’activité du matériel vidéo. |
| VCN / Vidéo | `/sys/bus/pci/devices/<PCI>/vcn_busy_percent` | Valeur sysfs utilisée directement | Pourcentage matériel du bloc VCN global, 0–100 %, encodage éventuellement inclus. |

Le tracker déduplique les descripteurs puis les clients partagés (PCI, pilote, identifiant DRM). Il additionne ensuite les deltas **entre clients distincts** et, pour une famille de l’accueil, entre noms d’instances reconnus. Cette somme reste un cumul : deux clients exécutés simultanément peuvent faire dépasser 1 000 ms/s. Les périodes en activité ne sont pas disponibles pour calculer leur union. Les capacités fdinfo, utiles pour normaliser une mesure individuelle, ne rétablissent pas cette information manquante dans l’agrégat de l’accueil.

Diviser les ms/s par dix produirait un indice cumulé en « % » pouvant dépasser 100, pas une occupation indépendante fiable. Changer d’unité seulement quand le résultat est inférieur à 100, ou plafonner les valeurs, introduirait une présentation trompeuse. Les trois courbes DRM restent donc temporairement en ms/s **même lorsque la valeur est petite ou nulle**. Aucun pourcentage estimé des clients n’est présenté comme une occupation matérielle.

`gpu_busy_percent` est une charge globale issue des IP du GPU, pas un compteur exclusivement 3D : il n’est pas réutilisé sous l’étiquette GFX/3D ou Compute. `mem_busy_percent` mesure le contrôleur mémoire. Le lecteur actuel ne dispose d’aucun autre compteur global indépendant et validé pour GFX, Compute ou Encode. L’interface binaire versionnée `gpu_metrics` n’est pas décodée dans cette tâche ; aucun nouveau backend n’est introduit.

Affichage : `0,0 %` est une mesure VCN valide ; « Non disponible » indique une source absente, inaccessible ou un premier relevé DRM ; « Mesure impossible » indique un pourcentage hors plage/non fini, un intervalle temporel invalide ou un compteur DRM régressif. Les interruptions sont conservées dans l’historique, sans effacer les points antérieurs. VCN conserve son axe fixe 0–100 % ; les courbes en ms/s restent non plafonnées.

Les infobulles de chaque graphique précisent la source, la formule et les limites de couverture. Références primaires : [statistiques DRM par client](https://www.kernel.org/doc/html/latest/gpu/drm-usage-stats.html), [compteurs globaux AMDGPU](https://www.kernel.org/doc/html/latest/gpu/amdgpu/thermal.html#busy-percent).
