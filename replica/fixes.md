# Corrections et angle : KRAKEN

Échantillon : **2 645 avis uniques** (2 719 lignes, 74 doublons retirés), collectés le 2026-10-17 par `replica/collect-reviews.py`, flux et API officiels seulement :

| source | avis | période |
| --- | --- | --- |
| App Store (flux RSS public), 5 pays : US, GB, CA, AU, FR | 2 500 lignes notées (943 à 1★, 191 à 2★, 213 à 3★, 227 à 4★, 926 à 5★), 2 426 après dédoublonnage | 2025-12 à 2026-10 |
| Hacker News (API Algolia), 5 requêtes | 119 commentaires | 2026-02 à 2026-10 |
| Tableau de suggestions de l'original (API Zendesk publique), 100 plus votés | 100 posts, avec leurs votes | 2018-11 à 2023-05 |

Trois types de source, donc ; `feedback.md` en compte 7 parce qu'il sépare les pays de l'App Store. Les décomptes ci-dessous viennent de recherches ciblées sur `reviews.csv` (motifs dans le texte, une ligne par avis) et sont des ordres de grandeur, pas des sondages : l'App Store pèse 92 % de l'échantillon et ses avis récents sont dominés par la vérification d'âge. Les thèmes « Teams and sharing » et « Recherche » de `feedback.md` sont du bruit (le mot *team* ou *threads* dans un autre sens) et ne sont pas repris. Citations en version originale, copiées telles quelles, avec leur lien.

## 1. Ce qu'ils détestent

1. **Devoir donner une pièce d'identité, un scan du visage ou une carte pour prouver son âge, et que ça échoue.** 101 avis, 3 sources (97 App Store, 3 HN, 1 tableau), 87 à 1 ou 2★.
   - "The face scanner is broken and put me in a “teens” category." ([App Store US](https://itunes.apple.com/us/rss/customerreviews/page=4/id=985746746/sortBy=mostRecent/json#14643223268))
   - "When take a look on how to verify… it said to use freaking credit card or face scan." ([App Store CA](https://itunes.apple.com/ca/rss/customerreviews/page=1/id=985746746/sortBy=mostRecent/json#14643533492))
2. **Payer pour l'essentiel, et voir l'abonnement poussé partout.** 207 avis, 3 sources : 163 citent l'abonnement ou les limites d'envoi (136 App Store, 15 HN, 12 tableau), 68 les pubs, quêtes et la boutique (certains les deux).
   - "WHY do you put the NITRO button in the text box with the most annoying bats animation ever??" ([App Store AU](https://itunes.apple.com/au/rss/customerreviews/page=1/id=985746746/sortBy=mostRecent/json#14650893205))
   - "Got nitro yet does that exclude me from half or even any of the ads on this platform?" ([App Store US](https://itunes.apple.com/us/rss/customerreviews/page=1/id=985746746/sortBy=mostRecent/json#14666811907))
3. **Les refontes imposées.** 74 avis, 3 sources (68 App Store, 2 HN, 4 tableau).
   - "The new UI looks bad, it doesn’t work and I can’t tap anything." ([App Store US](https://itunes.apple.com/us/rss/customerreviews/page=1/id=985746746/sortBy=mostRecent/json#14666552177))
   - "i really would love to go back to the old one." ([App Store US](https://itunes.apple.com/us/rss/customerreviews/page=4/id=985746746/sortBy=mostRecent/json#14646452167))
4. **Le numéro de téléphone exigé.** 63 avis, 2 sources (62 App Store, 1 HN).
   - "I don’t have a phone number for my gmail account" ([App Store CA](https://itunes.apple.com/ca/rss/customerreviews/page=1/id=985746746/sortBy=mostRecent/json#14656075269))
5. **Se retrouver bloqué hors de son compte** (boucles de captcha, « vérifie ton compte » sans fin). 52 avis, 2 sources (51 App Store, 1 HN).
   - "Im stuck in a register loop, i’ve tried everything." ([App Store US](https://itunes.apple.com/us/rss/customerreviews/page=2/id=985746746/sortBy=mostRecent/json#14663001050))
   - "It always says “verify your account first” even tho I ALREADY DID!" ([App Store US](https://itunes.apple.com/us/rss/customerreviews/page=1/id=985746746/sortBy=mostRecent/json#14667530864))
6. **Être banni sans explication ni humain pour faire appel.** 31 avis, 2 sources (29 App Store, 2 HN).
   - "Of how many times i try to send an appeal i can’t because i need to sign in. How can i sign in with a banned account." ([App Store GB](https://itunes.apple.com/gb/rss/customerreviews/page=8/id=985746746/sortBy=mostRecent/json#14210841746))
7. **Vocal et partage d'écran qui lâchent.** 45 avis, 3 sources (26 App Store, 8 HN, 11 tableau), plutôt des notes moyennes (13 à 1 ou 2★).
   - "i am NOT able to get the video or screenshare to pop back up again without ending and restarting the call." ([App Store US](https://itunes.apple.com/us/rss/customerreviews/page=4/id=985746746/sortBy=mostRecent/json#14647203966))
8. *Mince* : **un salon mis en muet notifie encore.** 4 avis, 2 sources.
   - "Why do I still get notifications when a channel has been muted?" ([App Store CA](https://itunes.apple.com/ca/rss/customerreviews/page=1/id=985746746/sortBy=mostRecent/json#14605280444))

## 2. Ce qui manque

Le tableau de suggestions donne les votes, mais ses posts les plus votés datent de 2018-2019 et l'original a livré depuis la plupart d'entre eux (sondages, balises spoiler, citations et réponses, dossiers de serveurs, statut personnalisé, avatar par serveur, test du micro, thèmes perso) : déduit de la connaissance générale du produit, journal des nouveautés non relu ici. Restent, sans livraison connue :

1. **Être invisible sur un serveur et en ligne sur un autre.** 8 146 votes, 1 source : *mince*. ([tableau](https://support.discord.com/hc/en-us/community/posts/360029554971-Be-invisible-on-one-server-but-online-on-another))
2. **Un tableau blanc partagé.** 10 739 votes, 1 source : *mince*. ([tableau](https://support.discord.com/hc/en-us/community/posts/360030042172-Whiteboard-Integration))
3. **Rouvrir l'appli là où on l'a laissée.** 2 avis, 1 source : *mince*, mais trivial.
   - "please add a setting to restore last location on launch." ([App Store US](https://itunes.apple.com/us/rss/customerreviews/page=8/id=985746746/sortBy=mostRecent/json#14611558420))

À noter contre nous : le post le plus voté du tableau (51 170 votes) **refuse le passage aux pseudos uniques** ([tableau](https://support.discord.com/hc/en-us/community/posts/14337329256983--Enshittification-DO-NOT-ADD-Discord-Username-System)), que KRAKEN reprend (pseudo unique + nom affiché). L'original l'a quand même imposé en 2023 ; à garder en tête pour replica-brand (ne pas en faire un argument) et pour un éventuel import de comptes.

## 3. Ce qui n'est pas résolu

1. **Les communautés qui veulent posséder leur espace** : héberger elles-mêmes, code ouvert, pas d'entreprise entre elles et leurs membres. 56 avis, 3 sources, surtout HN (43 HN, 7 App Store, 6 tableau).
   - "I'd love for an open source, self-hosted clone to pop up that keeps the user experience of Discord, but there is no other." ([HN](https://news.ycombinator.com/item?id=50088331))
   - "Find an alternative and organize a self host with your friends." ([App Store US](https://itunes.apple.com/us/rss/customerreviews/page=9/id=985746746/sortBy=mostRecent/json#14603866028))
2. **Les gens qui refusent de confier leur identité à une plateforme qui a déjà fui.** 40 avis sur la vie privée, 3 sources, et les fuites citées dans le thème 1.
   - "Horrible choice u leaked my info" ([App Store AU](https://itunes.apple.com/au/rss/customerreviews/page=8/id=985746746/sortBy=mostRecent/json#13740460183))

## Plan de corrections

Classées par preuve × coût. Chacune a sa ligne dans `features.csv` (`original = no`).

| # | quoi | taille | qui | preuve |
| --- | --- | --- | --- | --- |
| 1 | Aucune pub, boutique, quête ni bouton d'abonnement dans l'interface : tout est inclus. Règle de design écrite, vérifiée par un test e2e qui cherche ces mots. | S | replica-build | 207 avis, 3 sources |
| 2 | Inscription par e-mail et mot de passe seulement : jamais de téléphone, de pièce d'identité ni de scan du visage exigés par KRAKEN. Si une instance doit vérifier l'âge (loi locale), c'est un module optionnel de l'hébergeur, avec effacement des pièces. Question juridique par pays. | S (fait) + M (module) | replica-backend | 101 + 63 avis, 3 sources |
| 3 | Jamais bloqué dehors : codes de secours à l'activation de la MFA, réinitialisation par l'admin de l'instance, messages d'erreur qui disent quoi faire, pas de captcha en boucle. | M | replica-backend | 52 avis, 2 sources |
| 4 | Bannissement motivé : la personne voit le motif, l'auteur (modération du serveur ou admin de l'instance) et un formulaire de recours lu par un humain, accessible sans être connecté. | M | replica-build + backend | 31 avis, 2 sources |
| 5 | Pas de refonte imposée : rouvrir au dernier salon, densité et mise en page choisies gardées et synchronisées (`user_settings`). | S | replica-build | 74 + 2 avis, 3 sources |
| 6 | Auto-hébergement en une commande (fait : `docker compose up`) et export complet d'une communauté (salons, messages, rôles, membres) pour changer d'hébergeur. | M | replica-backend | 56 avis, 3 sources |
| 7 | Muet veut dire muet : un salon ou serveur en muet ne notifie plus, sauf mention directe si on l'a choisi. | S | replica-build | 4 avis, *mince* |

## L'angle

```
A. Pour ceux qui refusent de donner une pièce d'identité, leur visage ou leur numéro pour parler
   à leurs amis, KRAKEN demande un e-mail et un mot de passe, rien d'autre.
   Preuve : vérification d'âge et téléphone, 164 avis, 3 sources.

B. Pour ceux qui en ont assez qu'on leur vende un abonnement à chaque clic, KRAKEN donne tout
   à tout le monde, gratuitement, sans boutique ni pub.
   Preuve : abonnement, pubs et boutique, 207 avis, 3 sources.

C. Pour les communautés qui ne veulent plus qu'une entreprise décide à leur place (refontes,
   bannissements, données), KRAKEN est libre et tourne sur votre propre serveur : votre espace,
   vos règles.
   Preuve : auto-hébergement et code ouvert 56 avis, refontes 74, bannissements 31 ; 3 sources.
```

**Recommandé : C.** C'est le seul angle que l'original ne peut pas copier, il couvre A et B (une instance sans entreprise derrière n'a rien à vendre ni d'identité à collecter), et c'est la raison d'être du projet (libre, auto-hébergeable). A est la douleur la plus vive du moment mais risquée seule : la vérification d'âge vient de lois (Royaume-Uni, Australie, plusieurs États américains) qui s'appliqueront aussi aux instances publiques ; KRAKEN peut promettre qu'il ne collecte rien lui-même, pas que l'hébergeur n'aura jamais à le faire. À faire valider par un juriste avant d'en parler en public.

Ne pas utiliser le nom de l'original dans le nom, les pubs ou les fiches d'appli. Les avis cités ici sont de la recherche, pas des témoignages.
