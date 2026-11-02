## Télécharger

| Système | Fichier |
| --- | --- |
| Windows | [KRAKEN-Windows.exe](https://github.com/KAYO74/Kraken/releases/download/@TAG@/KRAKEN-Windows.exe) |
| macOS, puce Apple (M1, M2…) | [KRAKEN-macOS-Apple.dmg](https://github.com/KAYO74/Kraken/releases/download/@TAG@/KRAKEN-macOS-Apple.dmg), puis la commande ci-dessous |
| macOS, puce Intel | [KRAKEN-macOS-Intel.dmg](https://github.com/KAYO74/Kraken/releases/download/@TAG@/KRAKEN-macOS-Intel.dmg), puis la commande ci-dessous |
| Linux | [AppImage](https://github.com/KAYO74/Kraken/releases/download/@TAG@/KRAKEN-Linux.AppImage) (toutes distributions), [.deb](https://github.com/KAYO74/Kraken/releases/download/@TAG@/KRAKEN-Linux.deb) (Debian, Ubuntu) ou [.rpm](https://github.com/KAYO74/Kraken/releases/download/@TAG@/KRAKEN-Linux.rpm) (Fedora) |

**macOS** : l'appli n'est pas signée par Apple, donc macOS refuse de l'ouvrir. Télécharge le `.dmg` de ta puce (menu Pomme, « À propos de ce Mac » : « Puce Apple M… » ou « Processeur Intel »), ouvre Terminal et colle la commande qui va avec : elle installe KRAKEN dans Applications, lève le blocage et le lance.

Puce Apple (M1, M2, M3…) :

```sh
find /Volumes -maxdepth 1 -name 'KRAKEN*' -exec hdiutil detach -quiet {} \; 2>/dev/null; f="$(ls -t ~/Downloads/KRAKEN-macOS-Apple*.dmg | head -1)" && m="$(mktemp -d)" && hdiutil attach -nobrowse -quiet "$f" -mountpoint "$m" && rm -rf /Applications/KRAKEN.app && ditto "$m"/*.app /Applications/KRAKEN.app && hdiutil detach -quiet "$m" && xattr -cr /Applications/KRAKEN.app && open /Applications/KRAKEN.app
```

Puce Intel :

```sh
find /Volumes -maxdepth 1 -name 'KRAKEN*' -exec hdiutil detach -quiet {} \; 2>/dev/null; f="$(ls -t ~/Downloads/KRAKEN-macOS-Intel*.dmg | head -1)" && m="$(mktemp -d)" && hdiutil attach -nobrowse -quiet "$f" -mountpoint "$m" && rm -rf /Applications/KRAKEN.app && ditto "$m"/*.app /Applications/KRAKEN.app && hdiutil detach -quiet "$m" && xattr -cr /Applications/KRAKEN.app && open /Applications/KRAKEN.app
```

Déjà glissé dans Applications ? `xattr -cr /Applications/KRAKEN.app && open /Applications/KRAKEN.app` suffit.

**Windows** : SmartScreen peut avertir (appli non signée) : « Informations complémentaires », puis « Exécuter quand même ». Un `.msi` est aussi fourni pour les parcs gérés.

Au premier lancement, rejoins un serveur (son adresse ou un lien d'invitation) ou héberge le tien sur cet ordinateur en un clic.
