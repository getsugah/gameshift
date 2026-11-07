# GameShift 0.5.0-alpha.2

Première distribution publique alpha. Télécharger l’AppImage et son fichier
`.sha256` ci-dessus (bouton de téléchargement du fichier), puis suivre le README
à la racine du dépôt. L’archive `sources.tar.gz` fournit les sources de cette version
sans historique de développement. Elles sont également consultables à la racine.

Le menu de l’icône propose désormais **Quitter GameShift** : fermeture de
l’interface et arrêt propre de l’agent, avec conservation des profils. Mettre à jour
les contrôles depuis l’application si un ancien agent est installé.

Cette version propose la surveillance CPU/GPU, les tests Aurelia, les processus
DRM et les profils/réglages AMD facultatifs. Les détails sont dans CHANGELOG.md.
Aucun support, maintenance ou ajout de fonctionnalités prévu. Redistribution et
modification possibles selon la GPL-3.0-or-later et les licences tierces.

Reconstruction avec remappage des chemins privés et correction des répertoires
X11 générés. Le README vulgarisé, la licence du projet et les notices de 159 paquets
Rust utilisés sur Linux accompagnent l’AppImage. Les notices des bibliothèques
natives et des icônes sont conservées. Ne pas supprimer les copyrights tiers.

`NATIVE-COMPONENTS.json` identifie les bibliothèques natives, leurs empreintes et
les versions Debian exactes. Les liens snapshot donnent accès aux paquets binaires
et à leurs sources correspondantes ; consulter aussi les notices intégrées dans
`usr/share/licenses/`. Ces composants gardent leurs licences. Les sources amont du
runtime AppImage sont disponibles sur https://github.com/AppImage/type2-runtime.

SHA-256, sources, contenu de l’AppImage et cinq commandes en lecture seule vérifiés.
Les clés de test intégrées à GnuTLS ont été comparées à ses vecteurs publics.
Aucune identité Git ni aucun historique du dépôt Gitea n’a été importé.
Les essais graphiques et matériels Fedora et autres distributions restent à effectuer ; les
contrôles logiciels ne constituent pas une validation de ces plateformes.
