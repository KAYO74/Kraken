//! Les e-mails du serveur, dans la langue de l'interface de la personne : confirmation de l'adresse à l'inscription,
//! nouveau mot de passe, confirmation d'une nouvelle adresse. Texte brut, sans I/O : le serveur (`jobs.rs`) donne la
//! langue (`Lang`, lue dans la demande du client), le pseudo et le lien, et envoie le sujet et le corps tels quels.
//!
//! Le **français** est le texte d'origine (jamais modifié ici) ; l'anglais en suit le contenu. Le **persan** est neutre et
//! poli (forme « شما » sous-entendue, impératifs polis), avec les conventions de `crate::i18n` : chiffres latins (0-9),
//! lettres persanes « ی » et « ک », demi-espace (U+200C) entre deux lettres seulement, ponctuation « ، ؟ ؛ », aucune
//! marque de sens d'écriture, « KRAKEN » en latin et le lien seul sur sa ligne.
//!
//! Ajouter une langue : une branche dans chacune des trois fonctions (le compilateur les montre une fois la variante
//! ajoutée à `Lang`) et les tests de ce fichier la couvrent seuls (`Lang::ALL`).

use crate::i18n::Lang;

/// Un e-mail prêt à partir : le sujet (une ligne) et le corps (texte brut, terminé par un retour à la ligne).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mail {
    pub subject: String,
    pub body: String,
}

/// Confirmation de l'adresse à l'inscription : le lien (valable 24 heures) confirme le compte.
pub fn verify_email(lang: Lang, username: &str, link: &str) -> Mail {
    let (subject, body) = match lang {
        Lang::Fr => (
            "Confirme ton adresse e-mail KRAKEN",
            format!(
                "Bonjour {username},\n\nPour confirmer cette adresse, ouvre ce lien dans les 24 heures :\n{link}\n\n\
                 Tu n'as pas créé de compte KRAKEN ? Tu peux ignorer ce message.\n"
            ),
        ),
        Lang::En => (
            "Confirm your KRAKEN email address",
            format!(
                "Hello {username},\n\nTo confirm this address, open this link within 24 hours:\n{link}\n\n\
                 Didn't create a KRAKEN account? You can ignore this message.\n"
            ),
        ),
        Lang::Fa => (
            "نشانی ایمیل KRAKEN خود را تأیید کنید",
            format!(
                "سلام {username}،\n\nبرای تأیید این نشانی، این پیوند را ظرف 24 ساعت باز کنید:\n{link}\n\n\
                 اگر حساب KRAKEN نساخته‌اید، می‌توانید این پیام را نادیده بگیرید.\n"
            ),
        ),
    };
    Mail { subject: subject.into(), body }
}

/// Mot de passe oublié : le lien (valable 30 minutes, une seule fois) mène au choix d'un nouveau mot de passe.
pub fn reset_password(lang: Lang, username: &str, link: &str) -> Mail {
    let (subject, body) = match lang {
        Lang::Fr => (
            "Nouveau mot de passe KRAKEN",
            format!(
                "Bonjour {username},\n\nQuelqu'un, sans doute toi, a demandé un nouveau mot de passe pour ce compte. \
                 Ce lien sert une seule fois, pendant 30 minutes :\n{link}\n\n\
                 Si ce n'est pas toi, ne fais rien : ton mot de passe ne change pas.\n"
            ),
        ),
        Lang::En => (
            "Reset your KRAKEN password",
            format!(
                "Hello {username},\n\nSomeone, probably you, asked for a new password for this account. \
                 This link works once, for 30 minutes:\n{link}\n\n\
                 If it wasn't you, do nothing: your password stays the same.\n"
            ),
        ),
        Lang::Fa => (
            "بازنشانی رمز عبور KRAKEN",
            format!(
                "سلام {username}،\n\nکسی، احتمالاً خودتان، برای این حساب رمز عبور جدیدی درخواست کرده است. \
                 این پیوند فقط یک بار و به مدت 30 دقیقه معتبر است:\n{link}\n\n\
                 اگر این درخواست از طرف شما نبوده است، کاری نکنید؛ رمز عبور شما تغییر نمی‌کند.\n"
            ),
        ),
    };
    Mail { subject: subject.into(), body }
}

/// Changement d'adresse : le lien (valable 24 heures) part vers la nouvelle adresse et la confirme.
pub fn email_change(lang: Lang, username: &str, link: &str) -> Mail {
    let (subject, body) = match lang {
        Lang::Fr => (
            "Confirme ta nouvelle adresse e-mail KRAKEN",
            format!(
                "Bonjour {username},\n\nPour confirmer cette nouvelle adresse e-mail pour ton compte KRAKEN, ouvre ce lien dans les 24 heures :\n{link}\n\n\
                 Tu n'as rien demandé ? Ignore ce message : ton compte garde son adresse actuelle.\n"
            ),
        ),
        Lang::En => (
            "Confirm your new KRAKEN email address",
            format!(
                "Hello {username},\n\nTo confirm this new email address for your KRAKEN account, open this link within 24 hours:\n{link}\n\n\
                 Didn't ask for this? Ignore this message: your account keeps its current address.\n"
            ),
        ),
        Lang::Fa => (
            "نشانی ایمیل جدید KRAKEN خود را تأیید کنید",
            format!(
                "سلام {username}،\n\nبرای تأیید این نشانی ایمیل جدید برای حساب KRAKEN خود، این پیوند را ظرف 24 ساعت باز کنید:\n{link}\n\n\
                 درخواستی نداده‌اید؟ این پیام را نادیده بگیرید؛ حساب شما با نشانی فعلی خود باقی می‌ماند.\n"
            ),
        ),
    };
    Mail { subject: subject.into(), body }
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &str = "lea_7";
    const LINK: &str = "https://kraken.example.org/verify-email?token=Zx9-_aB3Zx9-_aB3Zx9-_aB3Zx9-_aB3Zx9-_aB3Zx9-_aB3";

    type Make = fn(Lang, &str, &str) -> Mail;
    const MAILS: [(&str, Make); 3] =
        [("verify_email", verify_email), ("reset_password", reset_password), ("email_change", email_change)];

    /// Les marques de sens d'écriture : jamais dans un e-mail (le lecteur isole le texte de lui-même).
    const BIDI: [char; 12] = [
        '\u{200E}', '\u{200F}', '\u{061C}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}', '\u{2066}',
        '\u{2067}', '\u{2068}', '\u{2069}',
    ];

    fn arabic_letter(c: char) -> bool {
        c.is_alphabetic() && ('\u{0600}'..='\u{06FF}').contains(&c)
    }

    #[test]
    fn every_mail_in_every_language_has_the_link_the_name_and_the_brand() {
        for lang in Lang::ALL {
            for (name, make) in MAILS {
                let m = make(lang, USER, LINK);
                let what = format!("{name} ({lang:?})");
                assert!(m.body.lines().any(|l| l == LINK), "{what} : le lien tient seul sur sa ligne\n{}", m.body);
                assert_eq!(m.body.matches(LINK).count(), 1, "{what} : le lien une seule fois");
                assert!(m.body.contains(USER), "{what} : le pseudo");
                assert!(m.subject.contains("KRAKEN"), "{what} : KRAKEN dans le sujet : {}", m.subject);
                assert!(m.body.ends_with('\n') && !m.body.ends_with("\n\n"), "{what} : un seul retour final");
                assert!(!m.subject.is_empty() && !m.subject.contains('\n'), "{what} : le sujet tient sur une ligne");
                assert!(!m.body.contains('\r'), "{what} : retours à la ligne `\\n`, jamais `\\r`");
                for text in [&m.subject, &m.body] {
                    assert!(!text.contains(['{', '}']), "{what} : variable non remplie\n{text}");
                }
                assert!(m.body.starts_with(match lang {
                    Lang::Fr => "Bonjour ",
                    Lang::En => "Hello ",
                    Lang::Fa => "سلام ",
                }));
            }
        }
    }

    #[test]
    fn the_durations_are_in_latin_digits_in_every_language() {
        for lang in Lang::ALL {
            for (name, make) in MAILS {
                let m = make(lang, USER, LINK);
                let n = if name == "reset_password" { "30" } else { "24" };
                let prose: String = m.body.lines().filter(|l| *l != LINK).collect::<Vec<_>>().join("\n");
                assert!(prose.contains(n), "{name} ({lang:?}) : {n} dans le texte\n{prose}");
            }
        }
    }

    #[test]
    fn the_three_languages_say_different_things() {
        for (name, make) in MAILS {
            let [fr, en, fa] = Lang::ALL.map(|l| make(l, USER, LINK));
            assert_ne!(fr.subject, en.subject, "{name}");
            assert_ne!(fr.subject, fa.subject, "{name}");
            assert_ne!(en.subject, fa.subject, "{name}");
            assert_ne!(fr.body, en.body, "{name}");
            assert_ne!(fr.body, fa.body, "{name}");
            assert_ne!(en.body, fa.body, "{name}");
            // Une langue ne reprend pas le texte d'une autre : pas d'accent français dans l'anglais, pas de latin
            // (hors KRAKEN, le pseudo et le lien) dans le persan.
            assert!(!en.body.contains(['é', 'è', 'à', 'ç']), "{name} : {}", en.body);
            assert!(fa.subject.chars().any(arabic_letter), "{name} : {}", fa.subject);
            let latin_words: Vec<String> = fa
                .body
                .lines()
                .filter(|l| *l != LINK)
                .flat_map(|l| l.split(|c: char| !c.is_ascii_alphabetic()).map(str::to_string).collect::<Vec<_>>())
                .filter(|w| !w.is_empty() && w != "KRAKEN" && w != "lea")
                .collect();
            assert!(latin_words.is_empty(), "{name} : mots latins dans le persan : {latin_words:?}");
        }
    }

    #[test]
    fn a_language_is_never_mixed_with_the_scripts_of_persian() {
        for lang in Lang::ALL {
            for (name, make) in MAILS {
                let m = make(lang, USER, LINK);
                let what = format!("{name} ({lang:?})");
                for text in [&m.subject, &m.body] {
                    for c in text.chars() {
                        assert!(
                            !('\u{06F0}'..='\u{06F9}').contains(&c) && !('\u{0660}'..='\u{0669}').contains(&c),
                            "{what} : chiffre persan ou arabo-indien {c:?} (chiffres latins seulement)\n{text}"
                        );
                        assert!(c != '\u{064A}' && c != '\u{0643}', "{what} : « ي » ou « ك » arabes {c:?}\n{text}");
                        assert!(!BIDI.contains(&c), "{what} : marque de sens d'écriture U+{:04X}\n{text}", c as u32);
                        assert!(c != '\u{FEFF}' && c != '\u{200B}' && c != '\u{200D}', "{what} : U+{:04X}", c as u32);
                    }
                    // Demi-espace : seulement entre deux lettres arabes.
                    let chars: Vec<char> = text.chars().collect();
                    for (i, c) in chars.iter().enumerate() {
                        if *c == '\u{200C}' {
                            let before = i.checked_sub(1).map(|j| chars[j]);
                            let after = chars.get(i + 1).copied();
                            assert!(
                                before.is_some_and(arabic_letter) && after.is_some_and(arabic_letter),
                                "{what} : demi-espace hors de deux lettres arabes ({before:?}, {after:?})\n{text}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn persian_uses_persian_punctuation_and_letters() {
        for (name, make) in MAILS {
            let m = make(Lang::Fa, USER, LINK);
            // Hors du lien : ni « , » ni « ? » ni « ; » latins (« ، » « ؟ » « ؛ »), et le mot « ی » persan est bien là.
            for line in m.body.lines().filter(|l| *l != LINK) {
                assert!(!line.contains([',', '?', ';']), "{name} : ponctuation latine dans « {line} »");
            }
            assert!(m.body.contains('،'), "{name} : la virgule persane");
            assert!(m.body.contains('ی') && !m.body.contains('ي'), "{name} : ی persan");
            // Le lien est seul sur sa ligne, sans espace ni texte autour.
            let at = m.body.lines().position(|l| l == LINK).expect("le lien");
            let lines: Vec<&str> = m.body.lines().collect();
            assert!(at > 0 && lines[at - 1].ends_with(':') && lines.get(at + 1) == Some(&""), "{name} : {lines:?}");
        }
    }

    /// Le français est le texte d'origine de `jobs.rs`, mot pour mot : il ne bouge pas.
    #[test]
    fn french_is_word_for_word_the_text_the_server_always_sent() {
        let verify = verify_email(Lang::Fr, "lea", "https://kraken.example.org/verify-email?token=abc");
        assert_eq!(verify.subject, "Confirme ton adresse e-mail KRAKEN");
        assert_eq!(
            verify.body,
            "Bonjour lea,\n\nPour confirmer cette adresse, ouvre ce lien dans les 24 heures :\n\
             https://kraken.example.org/verify-email?token=abc\n\n\
             Tu n'as pas créé de compte KRAKEN ? Tu peux ignorer ce message.\n"
        );

        let reset = reset_password(Lang::Fr, "lea", "https://kraken.example.org/reset-password?token=abc");
        assert_eq!(reset.subject, "Nouveau mot de passe KRAKEN");
        assert_eq!(
            reset.body,
            "Bonjour lea,\n\nQuelqu'un, sans doute toi, a demandé un nouveau mot de passe pour ce compte. \
             Ce lien sert une seule fois, pendant 30 minutes :\n\
             https://kraken.example.org/reset-password?token=abc\n\n\
             Si ce n'est pas toi, ne fais rien : ton mot de passe ne change pas.\n"
        );

        let change = email_change(Lang::Fr, "lea", "https://kraken.example.org/verify-email?token=abc");
        assert_eq!(change.subject, "Confirme ta nouvelle adresse e-mail KRAKEN");
        assert_eq!(
            change.body,
            "Bonjour lea,\n\nPour confirmer cette nouvelle adresse e-mail pour ton compte KRAKEN, ouvre ce lien dans les 24 heures :\n\
             https://kraken.example.org/verify-email?token=abc\n\n\
             Tu n'as rien demandé ? Ignore ce message : ton compte garde son adresse actuelle.\n"
        );
    }

    #[test]
    fn english_keeps_the_content_of_the_french() {
        let en = verify_email(Lang::En, "lea", "L");
        assert_eq!(en.subject, "Confirm your KRAKEN email address");
        assert!(en.body.contains("24 hours") && en.body.contains("ignore this message"));
        let en = reset_password(Lang::En, "lea", "L");
        assert_eq!(en.subject, "Reset your KRAKEN password");
        assert!(en.body.contains("30 minutes") && en.body.contains("works once"));
        assert!(en.body.contains("do nothing") && en.body.contains("password stays the same"));
        let en = email_change(Lang::En, "lea", "L");
        assert_eq!(en.subject, "Confirm your new KRAKEN email address");
        assert!(en.body.contains("24 hours") && en.body.contains("current address"));
    }

    #[test]
    fn persian_says_24_hours_and_30_minutes() {
        assert!(verify_email(Lang::Fa, "lea", "L").body.contains("24 ساعت"));
        assert!(email_change(Lang::Fa, "lea", "L").body.contains("24 ساعت"));
        assert!(reset_password(Lang::Fa, "lea", "L").body.contains("30 دقیقه"));
    }
}
