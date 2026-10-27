## Télécharger

| Système | Fichier |
| --- | --- |
| Windows | [KRAKEN-Windows.exe](https://github.com/KAYO74/Kraken/releases/download/@TAG@/KRAKEN-Windows.exe) |
| macOS (Intel et Apple Silicon) | [KRAKEN-macOS.dmg](https://github.com/KAYO74/Kraken/releases/download/@TAG@/KRAKEN-macOS.dmg), puis la commande ci-dessous |
| Linux | [AppImage](https://github.com/KAYO74/Kraken/releases/download/@TAG@/KRAKEN-Linux.AppImage) (toutes distributions), [.deb](https://github.com/KAYO74/Kraken/releases/download/@TAG@/KRAKEN-Linux.deb) (Debian, Ubuntu) ou [.rpm](https://github.com/KAYO74/Kraken/releases/download/@TAG@/KRAKEN-Linux.rpm) (Fedora) |

**macOS** : l'appli n'est pas signée par Apple, donc macOS refuse de l'ouvrir. Une fois le `.dmg` téléchargé, ouvre Terminal et colle cette commande : elle installe KRAKEN dans Applications, lève le blocage et le lance.

```sh
hdiutil attach -nobrowse -quiet ~/Downloads/KRAKEN-macOS.dmg -mountpoint /tmp/kraken && rm -rf /Applications/KRAKEN.app && ditto /tmp/kraken/KRAKEN.app /Applications/KRAKEN.app && hdiutil detach -quiet /tmp/kraken && xattr -cr /Applications/KRAKEN.app && open /Applications/KRAKEN.app
```

Déjà glissé dans Applications ? `xattr -cr /Applications/KRAKEN.app && open /Applications/KRAKEN.app` suffit.

**Windows** : SmartScreen peut avertir (appli non signée) : « Informations complémentaires », puis « Exécuter quand même ». Un `.msi` est aussi fourni pour les parcs gérés.

Au premier lancement, rejoins un serveur (son adresse ou un lien d'invitation) ou héberge le tien sur cet ordinateur en un clic.
