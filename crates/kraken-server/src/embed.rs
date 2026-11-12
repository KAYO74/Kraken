//! Aperçus de liens : une tâche de fond lit les adresses du message (`kraken_core::markdown::links`),
//! récupère chaque page par le garde de `fetch`, en tire titre, description, nom du site et image
//! (Open Graph, sinon `<title>`), garde l'image en local (jamais d'image distante dans la page
//! d'un membre : elle trahirait son IP), puis diffuse un MESSAGE_UPDATE.

use std::path::PathBuf;

use kraken_db::Job;
use kraken_proto::*;
use url::Url;

use crate::AppState;
use crate::fetch::{self, Policy, Want};
use crate::media;

/// Côté au plus d'une image d'aperçu, et de l'icône d'un site.
const IMAGE_SIDE: u32 = 640;
const ICON_SIDE: u32 = 32;

/// Ce qu'une page dit d'elle-même.
#[derive(Debug, Default, PartialEq)]
pub struct Page {
    pub title: Option<String>,
    pub description: Option<String>,
    pub site_name: Option<String>,
    /// Adresse absolue de l'image et de l'icône.
    pub image: Option<String>,
    pub icon: Option<String>,
}

/// Programme la récupération des aperçus d'un message qui vient d'être écrit ou modifié.
/// `had` : il en portait déjà (une modification qui retire le lien doit retirer l'aperçu).
pub async fn schedule(state: &AppState, mid: Id, content: &str, had: bool) {
    if !state.cfg.link_previews || (!had && kraken_core::markdown::links(content, MAX_EMBEDS).is_empty()) {
        return;
    }
    // Une tâche par état du texte : une modification pendant une récupération en lance une autre.
    let stamp = {
        use sha2::{Digest, Sha256};
        Sha256::digest(content.as_bytes()).iter().take(6).map(|b| format!("{b:02x}")).collect::<String>()
    };
    let key = format!("embed:{mid}:{stamp}");
    match state.db.enqueue("embed_fetch", serde_json::json!({ "message_id": mid }), Some(&key), 0).await {
        Ok(()) => state.jobs_wake.notify_one(),
        Err(e) => tracing::warn!("aperçus : tâche non créée : {e:?}"),
    }
}

/// La tâche `embed_fetch`.
pub async fn run(state: &AppState, job: &Job) -> Result<(), String> {
    let mid: Id = job
        .payload
        .get("message_id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .ok_or("message_id manquant")?;
    if !state.cfg.link_previews {
        return Ok(());
    }
    let Some((cid, content, old)) = state.db.message_embeds(mid).await.map_err(|e| format!("{e:?}"))? else {
        return Ok(()); // message supprimé entre-temps
    };
    let urls = kraken_core::markdown::links(&content, MAX_EMBEDS);
    let policy = Policy { allow_private: state.cfg.preview_allow_private };
    // Ceux déjà connus (même cachés) ne sont pas redemandés ; les autres, en parallèle.
    let mut fresh = Vec::new();
    for u in &urls {
        if !old.iter().any(|e| &e.url == u) {
            let (state, u) = (state.clone(), u.clone());
            fresh.push(tokio::spawn(async move { build(&state, &u, policy).await }));
        }
    }
    let mut built = Vec::new();
    for h in fresh {
        if let Ok(Some(e)) = h.await {
            built.push(e);
        }
    }
    // Dans l'ordre du texte.
    let next: Vec<Embed> = urls
        .iter()
        .filter_map(|u| {
            old.iter().find(|e| &e.url == u).cloned().or_else(|| built.iter().find(|e| &e.url == u).cloned())
        })
        .collect();
    if next == old {
        return Ok(());
    }
    let dropped: Vec<Embed> = old.iter().filter(|e| !next.iter().any(|n| n.url == e.url)).cloned().collect();
    let updated = state.db.set_embeds(mid, &next).await.map_err(|e| format!("{e:?}"))?;
    match updated {
        Some(m) => {
            crate::attach::remove_files(state, kraken_db::embed_paths(&dropped)).await;
            if let Ok(audience) = state.db.channel_audience(cid).await {
                state.hub.send(&audience, Event::MessageUpdate(Box::new(m)));
            }
        }
        // Le message a disparu pendant la récupération : ses images n'ont plus de maître.
        None => crate::attach::remove_files(state, kraken_db::embed_paths(&built)).await,
    }
    Ok(())
}

async fn build(state: &AppState, url: &str, policy: Policy) -> Option<Embed> {
    let page = match fetch::fetch(url, Want::Html, policy).await {
        Ok(p) => p,
        Err(e) => {
            tracing::debug!("aperçu de {url} : {e}");
            return None;
        }
    };
    let found = extract(&String::from_utf8_lossy(&page.body), &page.url);
    let image = match &found.image {
        Some(src) => store_image(state, src, IMAGE_SIDE, policy).await,
        None => None,
    };
    let favicon = match &found.icon {
        Some(src) => store_image(state, src, ICON_SIDE, policy).await.map(|i| i.key),
        None => None,
    };
    if found.title.is_none() && found.description.is_none() && image.is_none() {
        return None;
    }
    let site_name = found.site_name.or_else(|| {
        let host = page.url.host_str()?;
        Some(host.strip_prefix("www.").unwrap_or(host).to_string())
    });
    Some(Embed {
        url: url.to_string(),
        title: found.title,
        description: found.description,
        site_name,
        image,
        favicon,
        hidden: false,
    })
}

/// Récupère une image distante (même garde que la page), la réduit, la ré-encode et la range
/// sous `KRAKEN_DATA_DIR/embeds`.
async fn store_image(state: &AppState, src: &str, side: u32, policy: Policy) -> Option<EmbedImage> {
    let got = fetch::fetch(src, Want::Image, policy).await.ok()?;
    // Même portail que les photos de profil et les pièces jointes : deux décodages à la fois.
    let enc = media::gated(move || {
        let img = media::decode_within(&got.body, media::MEDIA).ok()?;
        media::shrink(&img, side)
    })
    .await??;
    let key = format!("{}.{}", crate::attach::new_key(), enc.ext);
    let dir: PathBuf = PathBuf::from(&state.cfg.data_dir).join("embeds");
    tokio::fs::create_dir_all(&dir).await.ok()?;
    tokio::fs::write(dir.join(&key), &enc.bytes).await.ok()?;
    Some(EmbedImage { key, width: enc.width, height: enc.height })
}

// ───────── lecture du HTML ─────────

fn squash(s: &str, max: usize) -> Option<String> {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        return None;
    }
    if t.chars().count() > max {
        let cut: String = t.chars().take(max - 1).collect();
        return Some(format!("{}…", cut.trim_end()));
    }
    Some(t)
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let decoded = rest.find(';').filter(|e| *e <= 10).and_then(|e| {
            let name = &rest[1..e];
            let c = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => ' ',
                "eacute" => 'é',
                "egrave" => 'è',
                "ecirc" => 'ê',
                "euml" => 'ë',
                "agrave" => 'à',
                "acirc" => 'â',
                "ccedil" => 'ç',
                "icirc" => 'î',
                "ocirc" => 'ô',
                "ugrave" => 'ù',
                "ucirc" => 'û',
                "hellip" => '…',
                "ndash" => '–',
                "mdash" => '—',
                "rsquo" => '’',
                "lsquo" => '‘',
                "laquo" => '«',
                "raquo" => '»',
                "copy" => '©',
                "euro" => '€',
                _ => {
                    let n = name.strip_prefix('#')?;
                    let code = match n.strip_prefix(['x', 'X']) {
                        Some(h) => u32::from_str_radix(h, 16).ok()?,
                        None => n.parse().ok()?,
                    };
                    char::from_u32(code)?
                }
            };
            Some((c, e + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Attributs d'une balise (noms en minuscules, valeurs sans entités), jusqu'au `>`.
/// Rend aussi la position après la balise.
fn attributes(tag: &str) -> (Vec<(String, String)>, usize) {
    let b = tag.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        match b[i] {
            b'>' => return (out, i + 1),
            c if c.is_ascii_whitespace() || c == b'/' => i += 1,
            _ => {
                let start = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
                    i += 1;
                }
                let name = tag[start..i].to_ascii_lowercase();
                while i < b.len() && b[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < b.len() && b[i] == b'=' {
                    i += 1;
                    while i < b.len() && b[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    let value = if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                        let q = b[i];
                        i += 1;
                        let s = i;
                        while i < b.len() && b[i] != q {
                            i += 1;
                        }
                        let v = &tag[s..i];
                        i = (i + 1).min(b.len());
                        v
                    } else {
                        let s = i;
                        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                            i += 1;
                        }
                        &tag[s..i]
                    };
                    out.push((name, unescape(value)));
                } else if !name.is_empty() {
                    out.push((name, String::new()));
                }
            }
        }
    }
    (out, b.len())
}

/// Lit l'en-tête d'une page : balises `<meta>` (Open Graph, Twitter, description), `<title>`,
/// `<link rel="icon">`. S'arrête au `<body>`. `base` : adresse finale de la page, pour les
/// adresses relatives.
pub fn extract(html: &str, base: &Url) -> Page {
    let lower = html.to_ascii_lowercase();
    let mut meta: Vec<(String, String)> = Vec::new();
    let mut title = None;
    let mut icon = None;
    let mut i = 0;
    while let Some(off) = lower[i..].find('<') {
        i += off;
        let rest = &lower[i..];
        if rest.starts_with("<!--") {
            i = lower[i..].find("-->").map_or(lower.len(), |e| i + e + 3);
            continue;
        }
        let name_len = rest[1..].find(|c: char| !(c.is_ascii_alphanumeric())).unwrap_or(rest.len() - 1);
        let name = &rest[1..1 + name_len];
        let after = i + 1 + name_len;
        match name {
            "body" => break,
            "script" | "style" | "noscript" => {
                let close = format!("</{name}");
                i = lower[after..].find(&close).map_or(lower.len(), |e| after + e + close.len());
            }
            "title" => {
                let end = lower[after..].find("</title").map_or(lower.len(), |e| after + e);
                if title.is_none()
                    && let Some(gt) = html[after..end].find('>')
                {
                    title = Some(unescape(&html[after + gt + 1..end]));
                }
                i = end;
            }
            "meta" | "link" => {
                let (attrs, used) = attributes(&html[after..]);
                let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
                if name == "meta" {
                    let key = get("property").or_else(|| get("name"));
                    if let (Some(k), Some(v)) = (key, get("content")) {
                        meta.push((k.to_ascii_lowercase(), v.to_string()));
                    }
                } else if icon.is_none()
                    && let (Some(rel), Some(href)) = (get("rel"), get("href"))
                    && rel.to_ascii_lowercase().split_whitespace().any(|r| r == "icon")
                {
                    icon = Some(href.to_string());
                }
                i = after + used;
            }
            _ => i = after,
        }
    }
    let first = |keys: &[&str]| -> Option<&str> {
        keys.iter().find_map(|k| meta.iter().find(|(n, v)| n == k && !v.trim().is_empty()).map(|(_, v)| v.as_str()))
    };
    let absolute = |s: &str| -> Option<String> {
        let u = base.join(s.trim()).ok()?;
        matches!(u.scheme(), "http" | "https").then(|| u.to_string())
    };
    Page {
        title: first(&["og:title", "twitter:title"]).map(String::from).or(title).and_then(|t| squash(&t, 150)),
        description: first(&["og:description", "twitter:description", "description"]).and_then(|d| squash(d, 300)),
        site_name: first(&["og:site_name"]).and_then(|s| squash(s, 60)),
        image: first(&["og:image", "og:image:url", "twitter:image", "twitter:image:src"]).and_then(absolute),
        icon: icon.as_deref().and_then(absolute),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"<!DOCTYPE html>
<html lang="fr"><head>
<meta charset="utf-8">
<title>  Titre   de la
 page &amp; co </title>
<!-- <meta property="og:title" content="commentaire"> -->
<script>var x = "<meta property='og:title' content='piège'>";</script>
<meta property="og:title" content="Un article &quot;vrai&quot;">
<meta property='og:description' content='Résumé &#233;crit
sur deux lignes'>
<meta property="og:site_name" content="Le Journal">
<meta property="og:image" content="/img/une.png">
<meta name="description" content="Autre description">
<link rel="shortcut icon" href="icone.png">
<link rel="stylesheet" href="s.css">
</head><body><meta property="og:title" content="dans le corps"><p>Salut</p></body></html>"#;

    #[test]
    fn extracts_open_graph_without_network() {
        let base = Url::parse("https://exemple.org/blog/article").unwrap();
        let p = extract(FIXTURE, &base);
        assert_eq!(p.title.as_deref(), Some("Un article \"vrai\""));
        assert_eq!(p.description.as_deref(), Some("Résumé écrit sur deux lignes"));
        assert_eq!(p.site_name.as_deref(), Some("Le Journal"));
        assert_eq!(p.image.as_deref(), Some("https://exemple.org/img/une.png"));
        assert_eq!(p.icon.as_deref(), Some("https://exemple.org/blog/icone.png"));
    }

    #[test]
    fn falls_back_to_title_and_description() {
        let base = Url::parse("http://exemple.org/").unwrap();
        let p = extract(
            "<html><head><TITLE>Ma  page &amp; moi</TITLE><META NAME=Description CONTENT=\"Courte\"></head>",
            &base,
        );
        assert_eq!(p.title.as_deref(), Some("Ma page & moi"));
        assert_eq!(p.description.as_deref(), Some("Courte"));
        assert_eq!((p.site_name, p.image, p.icon), (None, None, None));
        assert_eq!(extract("rien du tout", &base), Page::default());
        // Une image qui n'est pas du web est ignorée ; un texte trop long est coupé.
        let long = format!(
            "<meta property=\"og:image\" content=\"javascript:alert(1)\"><meta property=\"og:description\" content=\"{}\">",
            "a ".repeat(400)
        );
        let p = extract(&long, &base);
        assert_eq!(p.image, None);
        assert!(p.description.unwrap().chars().count() <= 300);
    }
}
