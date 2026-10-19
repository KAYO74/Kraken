#!/usr/bin/env python3
"""Collecte des avis publics sur l'original, uniquement par des flux et API officiels.

    python3 replica/collect-reviews.py > replica/reviews.csv

Sources (une source injoignable est signalée sur stderr et sautée, jamais remplacée) :
- App Store : flux RSS public des avis (Apple), plusieurs pays, 10 pages chacun ;
- Hacker News : API Algolia, commentaires sur quelques requêtes ;
- tableau de suggestions de l'original : API publique Zendesk de son centre d'aide.

Le texte est copié tel quel (le HTML est seulement converti en texte). Bibliothèque standard.
"""

import csv
import html
import json
import re
import sys
import urllib.parse
import urllib.request

APP_ID = "985746746"
COUNTRIES = ["us", "gb", "ca", "au", "fr"]
HN_QUERIES = ["discord alternative", "switched from discord", "leaving discord", "discord nitro", "discord sucks"]
BOARD = "https://support.discord.com/api/v2/community/posts.json?sort_by=votes&per_page=100"


def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": "kraken-research/0.1"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.load(r)


def text(markup):
    t = re.sub(r"<p>|<br\s*/?>", "\n", markup or "")
    return html.unescape(re.sub(r"<[^>]+>", "", t)).strip()


def app_store():
    for cc in COUNTRIES:
        for page in range(1, 11):
            url = f"https://itunes.apple.com/{cc}/rss/customerreviews/page={page}/id={APP_ID}/sortBy=mostRecent/json"
            entries = get(url).get("feed", {}).get("entry", [])
            if isinstance(entries, dict):
                entries = [entries]
            reviews = [e for e in entries if "im:rating" in e]
            if not reviews:
                break
            for e in reviews:
                yield {
                    "source": f"app-store-{cc}",
                    "url": f"{url}#{e['id']['label']}",
                    "date": e["updated"]["label"][:10],
                    "rating": e["im:rating"]["label"],
                    "title": e["title"]["label"],
                    "text": e["content"]["label"],
                }


def hacker_news():
    seen = set()
    for q in HN_QUERIES:
        url = "https://hn.algolia.com/api/v1/search_by_date?" + urllib.parse.urlencode(
            {"query": q, "tags": "comment", "hitsPerPage": 50}
        )
        for h in get(url).get("hits", []):
            body = text(h.get("comment_text"))
            if h["objectID"] in seen or "discord" not in body.lower():
                continue
            seen.add(h["objectID"])
            yield {
                "source": "hacker-news",
                "url": f"https://news.ycombinator.com/item?id={h['objectID']}",
                "date": h["created_at"][:10],
                "rating": "",
                "title": "",
                "text": body,
            }


def board():
    for p in get(BOARD).get("posts", []):
        yield {
            "source": "discord-feedback",
            "url": p["html_url"],
            "date": p["created_at"][:10],
            "rating": "",
            "title": p["title"],
            "text": f"{p['title']}\n{text(p.get('details'))}".strip(),
            "votes": p.get("vote_sum", ""),
        }


def main():
    out = csv.DictWriter(sys.stdout, ["source", "url", "date", "rating", "text", "title", "votes"])
    out.writeheader()
    for name, source in [("App Store", app_store), ("Hacker News", hacker_news), ("tableau de suggestions", board)]:
        n = 0
        try:
            for row in source():
                out.writerow(row)
                n += 1
        except Exception as e:  # source injoignable : on le dit, on continue
            print(f"{name} : injoignable après {n} lignes ({e})", file=sys.stderr)
            continue
        print(f"{name} : {n} lignes", file=sys.stderr)


if __name__ == "__main__":
    main()
