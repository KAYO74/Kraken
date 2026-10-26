//! Markdown des messages, sans HTML : le texte devient un arbre que le client dessine en
//! nœuds DOM (jamais de `innerHTML`) et dans lequel le serveur lit les mentions.
//!
//! Reconnu : `**gras**`, `*italique*` ou `_italique_`, `__souligné__`, `~~barré~~`,
//! `` `code` ``, blocs ```` ``` ````, citations `> `, `||spoiler||`, liens `http(s)://`
//! détectés seuls, mentions `@pseudo`, `@everyone`, `@here` et salons `#nom`. Une barre
//! oblique inverse rend littéral le signe qui la suit.

/// Un bloc du message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    /// Texte courant ; les retours à la ligne restent dans le texte.
    Text(Vec<Inline>),
    /// Lignes commençant par `> `.
    Quote(Vec<Inline>),
    /// Bloc de code entre ```` ``` ````, avec son langage s'il est donné sur la première ligne.
    Code { lang: Option<String>, text: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Bold(Vec<Inline>),
    Italic(Vec<Inline>),
    Underline(Vec<Inline>),
    Strike(Vec<Inline>),
    Spoiler(Vec<Inline>),
    Code(String),
    /// Adresse `http://` ou `https://`, rien d'autre.
    Link(String),
    /// `@pseudo` (pseudo unique, en minuscules).
    User(String),
    Everyone,
    Here,
    /// `#nom` d'un salon ; le client décide s'il existe.
    Channel(String),
}

/// Profondeur d'imbrication au plus (`**__~~…~~__**`) ; au-delà, le texte reste brut.
const MAX_DEPTH: usize = 6;
/// Mentions de personnes retenues au plus par message.
pub const MAX_MENTIONS: usize = 50;

pub fn parse(src: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find("```") {
        let after = &rest[i + 3..];
        let Some(j) = after.find("```") else { break };
        lines(&rest[..i], &mut blocks);
        let body = &after[..j];
        let (lang, text) = match body.split_once('\n') {
            Some((first, more)) if is_lang(first) => (Some(first.to_string()), more),
            Some((first, more)) if first.trim().is_empty() => (None, more),
            _ => (None, body),
        };
        blocks.push(Block::Code { lang, text: text.strip_suffix('\n').unwrap_or(text).to_string() });
        rest = &after[j + 3..];
        rest = rest.strip_prefix('\n').unwrap_or(rest);
    }
    lines(rest, &mut blocks);
    blocks
}

fn is_lang(s: &str) -> bool {
    (1..=20).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || "+-_#.".contains(c))
}

/// Regroupe les lignes consécutives : citations d'un côté, texte de l'autre.
fn lines(src: &str, out: &mut Vec<Block>) {
    let src = src.strip_suffix('\n').unwrap_or(src);
    if src.is_empty() {
        return;
    }
    let mut group: Vec<&str> = Vec::new();
    let mut quoting = false;
    let flush = |group: &mut Vec<&str>, quoting: bool, out: &mut Vec<Block>| {
        if group.is_empty() {
            return;
        }
        let inl = inline(&group.join("\n"), 0);
        out.push(if quoting { Block::Quote(inl) } else { Block::Text(inl) });
        group.clear();
    };
    for line in src.split('\n') {
        let quoted = line.strip_prefix("> ").or_else(|| (line == ">").then_some(""));
        if quoted.is_some() != quoting {
            flush(&mut group, quoting, out);
            quoting = quoted.is_some();
        }
        group.push(quoted.unwrap_or(line));
    }
    flush(&mut group, quoting, out);
}

fn word(c: Option<char>) -> bool {
    c.is_some_and(char::is_alphanumeric)
}

fn escapable(c: char) -> bool {
    c.is_ascii_punctuation()
}

/// Où se ferme `delim` dans `s` (position en octets), en sautant échappements et code.
fn close(s: &str, delim: &str) -> Option<usize> {
    let single = delim.len() == 1;
    let mut j = 0;
    while j < s.len() {
        let rest = &s[j..];
        if let Some(r) = rest.strip_prefix('\\')
            && let Some(c) = r.chars().next()
            && escapable(c)
        {
            j += 1 + c.len_utf8();
            continue;
        }
        if rest.starts_with('`')
            && let Some(k) = rest[1..].find('`')
        {
            j += k + 2;
            continue;
        }
        if rest.starts_with(delim) {
            if single && rest[1..].starts_with(delim) {
                // `**` dans un `*…*` : un autre marqueur, pas la fin.
                j += 2;
                continue;
            }
            if j > 0 {
                // `***` qui ferme un `**` : les deux dernières marques ferment.
                let mut k = j;
                while !single && s[k + 1..].starts_with(delim) {
                    k += 1;
                }
                return Some(k);
            }
        }
        j += rest.chars().next().map_or(1, char::len_utf8);
    }
    None
}

fn url_len(s: &str) -> usize {
    let mut end = s.find(|c: char| c.is_whitespace() || "<>\"".contains(c)).unwrap_or(s.len());
    while let Some(c) = s[..end].chars().next_back() {
        let unbalanced = c == ')' && s[..end].matches('(').count() < s[..end].matches(')').count();
        if ".,;:!?'*_~|".contains(c) || unbalanced {
            end -= c.len_utf8();
        } else {
            break;
        }
    }
    end
}

fn inline(s: &str, depth: usize) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut Vec<Inline>| {
        if !text.is_empty() {
            out.push(Inline::Text(std::mem::take(text)));
        }
    };
    let mut i = 0;
    'scan: while i < s.len() {
        let rest = &s[i..];
        let prev = s[..i].chars().next_back();
        let c = rest.chars().next().unwrap();
        if c == '\\'
            && let Some(n) = rest[1..].chars().next()
            && escapable(n)
        {
            text.push(n);
            i += 1 + n.len_utf8();
            continue;
        }
        if c == '`' {
            let n = if rest.starts_with("``") { 2 } else { 1 };
            if let Some(j) = rest[n..].find(&"``"[..n])
                && j > 0
            {
                flush(&mut text, &mut out);
                out.push(Inline::Code(rest[n..n + j].to_string()));
                i += j + 2 * n;
                continue;
            }
        }
        if depth < MAX_DEPTH {
            type Make = fn(Vec<Inline>) -> Inline;
            let marks: [(&str, Make); 6] = [
                ("||", Inline::Spoiler),
                ("**", Inline::Bold),
                ("__", Inline::Underline),
                ("~~", Inline::Strike),
                ("*", Inline::Italic),
                ("_", Inline::Italic),
            ];
            for (delim, make) in marks {
                if !rest.starts_with(delim) {
                    continue;
                }
                let d = delim.len();
                let Some(j) = close(&rest[d..], delim) else { continue };
                let inner = &rest[d..d + j];
                let after = rest[d + j + d..].chars().next();
                if d == 1 {
                    // Italique : pas d'espace collé aux marques ; `_` seulement entre deux mots.
                    let tight = !inner.starts_with(char::is_whitespace) && !inner.ends_with(char::is_whitespace);
                    if !tight || (delim == "_" && (word(prev) || word(after))) {
                        continue;
                    }
                }
                flush(&mut text, &mut out);
                out.push(make(inline(inner, depth + 1)));
                i += d + j + d;
                continue 'scan;
            }
        }
        if !word(prev) && (rest.starts_with("https://") || rest.starts_with("http://")) {
            let n = url_len(rest);
            if n > rest.find("//").unwrap() + 2 {
                flush(&mut text, &mut out);
                out.push(Inline::Link(rest[..n].to_string()));
                i += n;
                continue;
            }
        }
        if c == '@' && !word(prev) {
            let after = |n: usize| rest[n..].chars().next();
            for (kw, node) in [("@everyone", Inline::Everyone), ("@here", Inline::Here)] {
                if rest.starts_with(kw) && !word(after(kw.len())) {
                    flush(&mut text, &mut out);
                    out.push(node);
                    i += kw.len();
                    continue 'scan;
                }
            }
            let run = rest[1..]
                .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.'))
                .unwrap_or(rest.len() - 1);
            let name = rest[1..1 + run].trim_end_matches('.');
            if (2..=32).contains(&name.len()) && !word(after(1 + name.len())) {
                flush(&mut text, &mut out);
                out.push(Inline::User(name.to_string()));
                i += 1 + name.len();
                continue;
            }
        }
        if c == '#' && !word(prev) {
            let run =
                rest[1..].find(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_')).unwrap_or(rest.len() - 1);
            let name = &rest[1..1 + run];
            if (1..=100).contains(&name.chars().count()) {
                flush(&mut text, &mut out);
                out.push(Inline::Channel(name.to_string()));
                i += 1 + run;
                continue;
            }
        }
        text.push(c);
        i += c.len_utf8();
    }
    flush(&mut text, &mut out);
    out
}

/// Le texte sans ses marques (aperçu d'une réponse, spoiler encore masqué).
pub fn plain(inl: &[Inline]) -> String {
    let mut s = String::new();
    for i in inl {
        match i {
            Inline::Text(t) | Inline::Code(t) | Inline::Link(t) => s.push_str(t),
            Inline::Bold(v) | Inline::Italic(v) | Inline::Underline(v) | Inline::Strike(v) | Inline::Spoiler(v) => {
                s.push_str(&plain(v))
            }
            Inline::User(u) => {
                s.push('@');
                s.push_str(u);
            }
            Inline::Everyone => s.push_str("@everyone"),
            Inline::Here => s.push_str("@here"),
            Inline::Channel(c) => {
                s.push('#');
                s.push_str(c);
            }
        }
    }
    s
}

/// Tout le message en texte simple, les spoilers remplacés par des points de suspension.
pub fn summary(src: &str) -> String {
    fn hide(inl: &[Inline]) -> Vec<Inline> {
        inl.iter()
            .map(|i| match i {
                Inline::Spoiler(_) => Inline::Text("…".into()),
                Inline::Bold(v) => Inline::Bold(hide(v)),
                Inline::Italic(v) => Inline::Italic(hide(v)),
                Inline::Underline(v) => Inline::Underline(hide(v)),
                Inline::Strike(v) => Inline::Strike(hide(v)),
                other => other.clone(),
            })
            .collect()
    }
    parse(src)
        .iter()
        .map(|b| match b {
            Block::Text(v) | Block::Quote(v) => plain(&hide(v)),
            Block::Code { text, .. } => text.clone(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Qui le message mentionne : pseudos (sans doublon, [`MAX_MENTIONS`] au plus) et si
/// `@everyone` ou `@here` y figure. Rien n'est lu dans le code.
pub fn mentions(src: &str) -> (Vec<String>, bool) {
    fn walk(inl: &[Inline], users: &mut Vec<String>, all: &mut bool) {
        for i in inl {
            match i {
                Inline::User(u) => {
                    if users.len() < MAX_MENTIONS && !users.contains(u) {
                        users.push(u.clone());
                    }
                }
                Inline::Everyone | Inline::Here => *all = true,
                Inline::Bold(v) | Inline::Italic(v) | Inline::Underline(v) | Inline::Strike(v) | Inline::Spoiler(v) => {
                    walk(v, users, all)
                }
                _ => {}
            }
        }
    }
    let (mut users, mut all) = (Vec::new(), false);
    for b in parse(src) {
        if let Block::Text(v) | Block::Quote(v) = b {
            walk(&v, &mut users, &mut all);
        }
    }
    (users, all)
}

#[cfg(test)]
mod tests {
    use super::Inline::*;
    use super::*;

    fn t(s: &str) -> Inline {
        Text(s.into())
    }

    fn one(src: &str) -> Vec<Inline> {
        match parse(src).as_slice() {
            [Block::Text(v)] => v.clone(),
            other => panic!("un bloc de texte attendu : {other:?}"),
        }
    }

    #[test]
    fn emphasis() {
        assert_eq!(one("**gras** et *ita*"), vec![Bold(vec![t("gras")]), t(" et "), Italic(vec![t("ita")])]);
        assert_eq!(
            one("__sous__ ~~barré~~ _aussi_"),
            vec![Underline(vec![t("sous")]), t(" "), Strike(vec![t("barré")]), t(" "), Italic(vec![t("aussi")])]
        );
        assert_eq!(one("**gras *et ita***"), vec![Bold(vec![t("gras "), Italic(vec![t("et ita")])])]);
        assert_eq!(one("*a **b** c*"), vec![Italic(vec![t("a "), Bold(vec![t("b")]), t(" c")])]);
        // Pas de mise en forme au milieu d'un mot avec `_`, ni avec des espaces collés.
        assert_eq!(one("nom_de_variable"), vec![t("nom_de_variable")]);
        assert_eq!(one("2 * 3 * 4"), vec![t("2 * 3 * 4")]);
        assert_eq!(one("**seul"), vec![t("**seul")]);
        assert_eq!(one(r"\*\*pas gras\*\*"), vec![t("**pas gras**")]);
    }

    #[test]
    fn code_spoilers_and_links() {
        assert_eq!(one("`**brut**` ok"), vec![Code("**brut**".into()), t(" ok")]);
        assert_eq!(one("``a ` b``"), vec![Code("a ` b".into())]);
        assert_eq!(one("fin ||du film||"), vec![t("fin "), Spoiler(vec![t("du film")])]);
        assert_eq!(
            one("va sur https://exemple.org/a_b_c."),
            vec![t("va sur "), Link("https://exemple.org/a_b_c".into()), t(".")]
        );
        assert_eq!(
            one("(voir https://fr.wikipedia.org/wiki/Kraken_(mythologie))"),
            vec![t("(voir "), Link("https://fr.wikipedia.org/wiki/Kraken_(mythologie)".into()), t(")")]
        );
        assert_eq!(one("javascript:alert(1) et https:// seul"), vec![t("javascript:alert(1) et https:// seul")]);
        assert_eq!(one("**https://exemple.org**"), vec![Bold(vec![Link("https://exemple.org".into())])]);
    }

    #[test]
    fn blocks() {
        assert_eq!(
            parse("avant\n```rust\nfn main() {}\n```\naprès"),
            vec![
                Block::Text(vec![t("avant")]),
                Block::Code { lang: Some("rust".into()), text: "fn main() {}".into() },
                Block::Text(vec![t("après")]),
            ]
        );
        assert_eq!(parse("```un **seul** bloc```"), vec![Block::Code { lang: None, text: "un **seul** bloc".into() }]);
        assert_eq!(
            parse("> cité\n> encore\nrépondu"),
            vec![Block::Quote(vec![t("cité\nencore")]), Block::Text(vec![t("répondu")])]
        );
        assert_eq!(parse("```pas fermé"), vec![Block::Text(vec![t("```pas fermé")])]);
        assert_eq!(parse("ligne une\nligne deux"), vec![Block::Text(vec![t("ligne une\nligne deux")])]);
    }

    #[test]
    fn mentions_and_channels() {
        assert_eq!(
            one("salut @lou et @sam."),
            vec![t("salut "), User("lou".into()), t(" et "), User("sam".into()), t(".")]
        );
        assert_eq!(one("écris à moi@exemple.org"), vec![t("écris à moi@exemple.org")]);
        assert_eq!(one("@everyone @here @everyones"), vec![Everyone, t(" "), Here, t(" "), User("everyones".into())]);
        assert_eq!(one("@Lou"), vec![t("@Lou")]);
        assert_eq!(one("va dans #général, merci"), vec![t("va dans "), Channel("général".into()), t(", merci")]);
        assert_eq!(one("C# et #"), vec![t("C# et #")]);
        let (users, all) = mentions("@lou **@sam** `@noa` @lou ||@here||\n```\n@ilan\n```");
        assert_eq!(users, ["lou", "sam"]);
        assert!(all);
        assert_eq!(mentions("rien"), (vec![], false));
        assert_eq!(summary("**a** ||secret|| @lou"), "a … @lou");
    }

    #[test]
    fn odd_inputs_do_not_panic() {
        for s in
            ["", "*", "**", "|||", "``", "\\", "é*", "*é*", "@", "#", "> ", ">", "```", "______", "~~~~~", "@é", "#é"]
        {
            let _ = parse(s);
        }
        let deep = "**__~~||*".repeat(50) + "x" + &"*||~~__**".repeat(50);
        let _ = parse(&deep);
        let _ = parse(&"*a".repeat(2000));
    }
}
