# Corrections et angle : KRAKEN

Statut au 2026-10-17 : **collecte bloquée, aucune liste établie.** Échantillon : 0 avis.

Les trois sources prévues ne sont pas joignables depuis l'environnement cloud (le proxy refuse `itunes.apple.com`, `hn.algolia.com` et `support.discord.com`). Rien n'est donc avancé ici sur ce que détestent les utilisateurs de l'original : pas de liste, pas de plan, pas d'angle tant qu'il n'y a pas d'avis réels et liés.

Prêt pour la reprise :

- `scripts/collect-reviews.py` : collecte par flux et API officiels seulement (RSS des avis App Store pour 5 pays, API Algolia de Hacker News, API publique Zendesk du tableau de suggestions de l'original avec ses votes). Texte copié tel quel, une source injoignable est signalée et sautée.
- `replica/themes.json` : les thèmes de `reviews.py` plus ceux du domaine (abonnement payant, vocal, vérification d'identité, bans et spam, consommation mémoire, dépendance à une entreprise, recherche), en anglais et en français.
- `replica/reviews.csv` : en-tête seul.

Reprise, une fois ces trois domaines autorisés :

```sh
python3 scripts/collect-reviews.py > replica/reviews.csv
python3 <skill>/reviews.py replica/reviews.csv --themes replica/themes.json --out replica/feedback.md
```

Puis : les trois listes (ce qu'ils détestent, ce qui manque, ce qui n'est pas résolu), le plan de 5 à 8 corrections ajoutées à `features.csv` avec `original = no`, et trois angles de positionnement. Reddit demande une appli OAuth créée par le propriétaire : facultatif.
