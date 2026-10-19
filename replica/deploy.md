# Déploiement : KRAKEN

Date : 2026-10-19  Commit : voir la PR #4  Feu vert de Fred : pas encore

Hébergement retenu : un serveur Linux (VPS) avec `docker compose` : Postgres 18, KRAKEN, Caddy pour le HTTPS automatique, sauvegarde quotidienne. Vercel et équivalents ne conviennent pas : binaire Rust, WebSocket permanent, base PostgreSQL.

## Préalables (tous obligatoires)

- [x] e2e verts : 50 / 50 (Playwright, Chromium), plus 18 tests Rust
- [x] aucun bug S1 ou S2 ouvert (`replica/bugs.md` : tous les S1 et S2 corrigés)
- [ ] **parité : 2 fonctions indispensables sur 40, score 14,2 / 100** : bloquant. Manquent entre autres le vocal, la modération, les notifications, la recherche, les emojis (`replica/parity.md`).
- [x] rebranding propre : `sweep.py . --config replica/brand.json` sort 0 (nom, domaines et couleurs de l'original absents du code)
- [ ] fiche des magasins d'applications : sans objet tant qu'il n'y a pas d'appli mobile
- [x] build de production : image Docker construite par la CI, client en `release`
- [x] Confidentialité (`/privacy`) et Conditions (`/terms`) en ligne ; contact et sous-traitants renseignés par l'exploitant (`KRAKEN_CONTACT`, `KRAKEN_PROCESSORS`)
- [x] suppression de compte : marche, y compris pour un propriétaire de serveur
- [x] favicon, titre, image de partage, icônes : le logo KRAKEN de Fred

Verdict : **pas encore publiable comme service ouvert à tous** (parité). Une instance de test privée, sur invitation, est possible dès maintenant si Fred le décide.

## Production

- [x] base séparée : volume `db` de l'instance, jamais la base de développement
- [x] sauvegardes : service `backup`, `pg_dump` chaque jour dans `backups/`, 14 jours gardés
- [x] migrations au démarrage du serveur, pas à la main
- [ ] variables dans `.env` sur le serveur (noms dans `.env.example`) : à faire par Fred
- [x] paiements : aucun (tout est gratuit), pas de Stripe
- [x] OAuth : aucun pour l'instant
- [ ] domaine d'envoi des e-mails vérifié chez le fournisseur SMTP de Fred (sinon pas de mot de passe oublié)

## Domaine (une fois acheté par Fred)

| enregistrement | nom | valeur |
| --- | --- | --- |
| A | `@` ou le sous-domaine choisi (ex. `chat`) | IPv4 du serveur |
| AAAA | idem | IPv6 du serveur, s'il en a une |
| TXT | `@` | SPF donné par le fournisseur d'e-mails |
| TXT ou CNAME | selon le fournisseur | DKIM donné par le fournisseur d'e-mails |
| TXT | `_dmarc` | `v=DMARC1; p=none; rua=mailto:<adresse de Fred>` puis `p=quarantine` |

- [ ] hôte canonique : `KRAKEN_DOMAIN` seul ; un `www` éventuel se redirige dans `deploy/Caddyfile`
- [ ] HTTPS valide : Caddy obtient le certificat au premier démarrage si le DNS pointe déjà sur le serveur

## Mise en route (Fred, sur le serveur)

```sh
git clone https://github.com/KAYO74/Kraken && cd Kraken
cp .env.example .env     # KRAKEN_DOMAIN, POSTGRES_PASSWORD, KRAKEN_CONTACT, KRAKEN_PROCESSORS
docker compose up -d
```

Pare-feu : ouvrir 80 et 443 (TCP) et 443 (UDP) seulement. Le port 8080 n'est pas publié.

## Surveillance

- [ ] disponibilité : une sonde (ex. UptimeRobot, gratuit) sur `https://<domaine>/api/v1/health`, qui répond `ok` quand serveur et base vont bien, alerte par e-mail à Fred
- [x] journaux : `docker compose logs -f kraken`
- [x] mesure d'audience : aucune, par choix (pas de bandeau cookies)
- [ ] parcours principal fait sur le site en ligne, ordinateur et téléphone

## Première semaine

Les erreurs `base :` dans les journaux, la taille de `backups/`, les refus d'envoi SMTP (`refus SMTP` dans les journaux), et les 429 sur la connexion si une communauté entière arrive derrière une même adresse IP (relever `KRAKEN_AUTH_PER_MIN`).
