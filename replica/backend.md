# Backend : KRAKEN

Date : 2026-10-17. Tout tourne dans `kraken-server` (un binaire) sur PostgreSQL. Aucun service tiers obligatoire : l'admin d'instance branche seulement un SMTP s'il veut des e-mails.

## Base

- Migrations dans `crates/kraken-db/migrations/`, appliquées au démarrage du serveur (`sqlx::migrate!`). `0001_schema.sql` est la copie de `replica/schema.sql` ; on n'édite plus que de nouvelles migrations.
- `0000_uuidv7_compat.sql` : `uuidv7()` est natif en Postgres 18 ; pour 16 et 17 une fonction équivalente (triée à la sous-milliseconde) est créée. Testé ici sur Postgres 16, en CI sur 18.
- `0002_jobs_dead_letter.sql` : colonne `failed_at` pour les tâches abandonnées.
- Accès : pas de RLS (le serveur est le seul client). Chaque handler appelle le moteur de permissions (`kraken-core`) via `kraken-db` avant toute lecture ou écriture ; un non-membre reçoit 404, pas 403, pour ne pas révéler qu'un salon existe. Testé avec un second compte (`outsiders_see_nothing`, `invites_messages_and_private_channels`, test HTTP `register_create_invite_and_chat_live`).
- Données de démo : `kraken-server --seed` (ou `KRAKEN_SEED=1`) sur une base vide ; personnages fictifs, adresses en `.invalid`.
- Sauvegardes : à la charge de l'admin d'instance. Avec Docker : `docker compose exec db pg_dump -U kraken kraken > sauvegarde.sql` dans un cron quotidien. À documenter dans le guide d'admin.
- Écart à l'architecture : requêtes sqlx vérifiées **à l'exécution**, pas à la compilation (`query!`). Les tests sur vraie base couvrent chaque requête ; passer à `query!` + `.sqlx/` demandera `sqlx-cli` en CI.

## Comptes et sessions

- Inscription pseudo + e-mail + mot de passe (8 caractères mini), argon2id.
- Session : jeton aléatoire 256 bits, seul son SHA-256 est en base. Le client web le reçoit dans un cookie `HttpOnly; SameSite=Lax` (`Secure` dès que `KRAKEN_PUBLIC_URL` est en https) ; les clients natifs et les bots utilisent `Authorization: Session <jeton>`.
- Anti-CSRF : une écriture authentifiée par cookie doit porter un `Origin` égal à l'hôte ; même règle pour le cookie à l'ouverture du gateway.
- Déconnexion, déconnexion partout (`/auth/logout-all`, ferme aussi les gateways ouverts).
- Vérification d'e-mail (lien 24 h) et mot de passe oublié (lien 30 min, usage unique, ferme toutes les sessions). Jetons créés au moment de l'envoi, stockés hachés, jamais en clair dans la file. Sans SMTP : pas de vérification et `/auth/reset` répond 501 avec un message clair.
- Suppression de compte confirmée par mot de passe : fermeture immédiate (sessions révoquées, connexion refusée), anonymisation par la tâche `account_purge` après 14 jours (identité, e-mail, mot de passe, profil, amis, adhésions ; les messages restent sous « compte supprimé »). Un propriétaire de serveur doit d'abord le transférer.
- Pas encore : OAuth (Google, Apple…), MFA TOTP, liste des appareils, désactivation. Le recon ne montre pas de lien magique.

## Paiements

Aucun, par choix : tout KRAKEN est gratuit, fonctions « Nitro » comprises (voir `architecture.md`). Pas de Stripe, pas de webhooks entrants.

## E-mail et tâches

- SMTP via `lettre` (TLS rustls), `KRAKEN_SMTP_URL` + `KRAKEN_SMTP_FROM`. Textes écrits pour KRAKEN (vérification, mot de passe oublié). L'envoi réel n'a pas été testé ici (pas de SMTP dans ce bac à sable) ; la file, elle, est testée.
- File `jobs` : prise `FOR UPDATE SKIP LOCKED`, verrou de 5 min, reprise exponentielle (2^n minutes), lettre morte (`failed_at`) après 5 échecs, dédoublonnage par `dedupe_key`. Tâches : `email_verify`, `email_reset`, `account_purge` (quotidienne).
- Horaires en UTC (`timestamptz`), affichés en heure locale par le client.

## Intégrations

Aucune dans cette tranche. Plus tard (S15) : comptes liés via l'OAuth public de chaque plateforme, avec les clés de l'admin d'instance et les scopes minimaux ; aucune API privée de Discord, jamais.

## Variables

Toutes dans `.env.example` (sans valeurs). `docker-compose.yml` lance Postgres 18 et KRAKEN ; l'image est construite par la CI (pas de démon Docker dans ce bac à sable).

## Sécurité

- [x] secrets seulement dans l'environnement, `.env*` ignorés par git, aucun secret dans le client
- [x] entrées validées côté serveur sur chaque route (types serde + longueurs, formats, bornes)
- [x] autorisation vérifiée à chaque lecture et écriture, testée avec un second compte
- [x] limites de débit par IP sur connexion, inscription, vérif et mot de passe oublié (`KRAKEN_AUTH_PER_MIN`) ; pas encore sur l'envoi de messages
- [x] webhooks : sans objet (aucun webhook entrant)
- [ ] envois de fichiers : pas encore construits ; prévus servis depuis une origine séparée
- [x] pas de données perso dans les URL ni les journaux (seuls les liens d'e-mail portent un jeton, à usage unique et court)
- [x] dépendances auditées : voir ci-dessous
- [ ] politique de confidentialité : à écrire au lancement ; sous-traitants = l'hébergeur et le SMTP choisis par l'admin d'instance

## Audit des dépendances

`cargo audit` le 2026-10-17 : **0 vulnérabilité** sur le code compilé.
- RUSTSEC-2023-0071 (`rsa`) ignorée dans `.cargo/audit.toml` : la crate n'arrive que par `sqlx-mysql`, présent dans `Cargo.lock` mais jamais compilé (`cargo tree -i rsa --target all` est vide).
- Avertissement : `paste` n'est plus maintenu (macro utilisée par Leptos). Sans risque connu ; à suivre avec les mises à jour de Leptos.
