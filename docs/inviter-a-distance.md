# Inviter quelqu'un à distance

Ton KRAKEN tourne sur ton ordinateur (« Héberger sur cet ordinateur ») et la personne à inviter est ailleurs. Le plus simple et le plus sûr : [Tailscale](https://tailscale.com), un réseau privé chiffré entre vos deux ordinateurs. Gratuit pour un usage personnel, rien à ouvrir sur ta box, et ton KRAKEN reste invisible depuis Internet.

Il faut KRAKEN de bureau 0.5.0 ou plus, et un compte Tailscale chacun (un compte Google, Microsoft, Apple ou GitHub suffit).

## 1. Chez toi

1. Installe Tailscale ([tailscale.com/download](https://tailscale.com/download)) et connecte-toi.
2. Ouvre KRAKEN, puis « Héberger sur cet ordinateur ». Note le port à la fin de l'adresse affichée (souvent `8080`).
3. Laisse KRAKEN et Tailscale ouverts, et ton ordinateur allumé, tant que vous l'utilisez.

## 2. Partage ton ordinateur avec ton invité

Dans la [console Tailscale](https://login.tailscale.com/admin/machines), clique sur « … » à côté de ton ordinateur, puis « Share », et envoie-lui le lien. Il installe Tailscale, se connecte avec son propre compte, ouvre le lien et accepte. Il ne voit que cet ordinateur, rien d'autre de ton réseau.

## 3. Donne-lui une adresse en https

Le micro et le partage d'écran ne marchent que sur une adresse https. Tailscale en fournit une, avec un vrai certificat. Dans un terminal (remplace `8080` par ton port) :

```sh
tailscale serve --bg 8080
```

- macOS : si la commande est introuvable, tape `/Applications/Tailscale.app/Contents/MacOS/Tailscale serve --bg 8080`.
- La première fois, la commande donne un lien pour activer HTTPS sur ton compte : ouvre-le, accepte, puis relance la commande.
- Elle affiche ton adresse, du type `https://ton-ordi.tailxxxx.ts.net`. C'est celle-là que tu envoies.
- Pour arrêter le partage : `tailscale serve reset`.

## 4. Ton invité se connecte

1. Dans KRAKEN de bureau, « Rejoindre un serveur » avec l'adresse https (ou dans son navigateur).
2. Il crée son compte.
3. Toi : Amis, « Ajouter un ami », son @identifiant. Une fois la demande acceptée, vous pouvez vous écrire et vous appeler depuis le MP, avec partage d'écran.
4. Pour l'ajouter à un de tes serveurs : « Inviter » donne un lien avec ton adresse locale (`192.168…`), qui ne marche pas chez lui. Une fois connecté, il colle ce lien dans « + », rejoindre un serveur : seul le code à la fin compte.

Le son et l'image passent aussi par Tailscale : rien à ouvrir sur ta box.

## Si ça ne marche pas

- **L'adresse ne s'ouvre pas chez lui** : vérifiez que Tailscale est connecté des deux côtés. Sinon, invite-le plutôt dans ton réseau Tailscale (console, « Users », « Invite ») au lieu de partager l'ordinateur.
- **Sans https** : ton adresse Tailscale `http://100.x.y.z:8080` (menu Tailscale, « Copy IP ») marche aussi. Sous Windows et Linux, KRAKEN 0.5.0 y autorise micro et partage d'écran ; sur macOS, il pourra seulement écouter et regarder.
- **Pas de son** : le pare-feu de ton ordinateur doit laisser entrer KRAKEN (macOS le demande au premier lancement du serveur : « Autoriser »).

## Et Cloudflare Tunnel ?

`cloudflared tunnel --url http://localhost:8080` donne une adresse https publique en une commande, sans compte. Mais n'importe qui sur Internet peut l'ouvrir, et les appels et salons vocaux ne passent pas : le tunnel ne transporte pas le son et l'image (UDP). Bien pour essayer la messagerie, pas pour vous appeler.
