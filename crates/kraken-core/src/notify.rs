//! Non-lus, mentions et notifications : ce qui compte comme une mention pour quelqu'un, et ce
//! que ses réglages en font. Le serveur s'en sert pour tenir les compteurs, le client pour les
//! suivre en direct et décider d'une notification sur le bureau.

use uuid::Uuid;

/// Ce qui prévient : tous les messages, les mentions seulement, ou rien.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    All,
    Mentions,
    Nothing,
}

/// Niveau d'un serveur sans réglage : les mentions seulement (un serveur animé ne doit pas
/// sonner à chaque message). Les MP et les groupes préviennent toujours de tout.
pub const GUILD_DEFAULT: Level = Level::Mentions;

/// Le niveau qui s'applique à un salon : celui du salon s'il en a un, sinon celui de son
/// serveur, sinon [`GUILD_DEFAULT`] ; dans un MP ou un groupe, [`Level::All`] à défaut.
pub fn effective_level(private: bool, channel: Option<Level>, guild: Option<Level>) -> Level {
    channel.or(guild).unwrap_or(if private { Level::All } else { GUILD_DEFAULT })
}

/// Sourdine en cours à l'instant `now` (millisecondes) : `until` est sa fin, `i64::MAX` pour
/// « jusqu'à ce que je la retire ».
pub fn muted(until: Option<i64>, now: i64) -> bool {
    until.is_some_and(|u| u > now)
}

/// Un message, vu par celui qui pourrait en être prévenu.
#[derive(Clone, Copy, Debug)]
pub struct Seen<'a> {
    /// `None` : compte supprimé.
    pub author: Option<Uuid>,
    /// Personnes nommées par `@pseudo`.
    pub mentions: &'a [Uuid],
    /// `@everyone` ou `@here` écrit par quelqu'un qui en a le droit.
    pub everyone: bool,
    /// Auteur du message auquel celui-ci répond.
    pub reply_to: Option<Uuid>,
    /// MP ou groupe : tout message y compte.
    pub private: bool,
}

/// Le message compte-t-il comme une mention pour `me` ? `@pseudo`, `@everyone`/`@here` (sauf
/// si `me` les ignore dans ce serveur), une réponse à l'un de ses messages, tout message d'un MP
/// ou d'un groupe. Jamais ses propres messages.
pub fn is_mention(me: Uuid, m: &Seen<'_>, ignore_everyone: bool) -> bool {
    if m.author == Some(me) {
        return false;
    }
    m.private || m.mentions.contains(&me) || (m.everyone && !ignore_everyone) || m.reply_to == Some(me)
}

/// Le compteur de mentions avance-t-il ? Oui pour une mention, sauf au niveau « rien »
/// (la sourdine, elle, n'arrête pas le compteur).
pub fn counts(level: Level, mention: bool) -> bool {
    mention && level != Level::Nothing
}

/// Faut-il prévenir (notification sur le bureau) ? Jamais en sourdine ; sinon selon le niveau.
pub fn notifies(level: Level, muted: bool, mention: bool) -> bool {
    !muted
        && match level {
            Level::All => true,
            Level::Mentions => mention,
            Level::Nothing => false,
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> Uuid {
        Uuid::from_bytes([n; 16])
    }

    #[test]
    fn what_counts_as_a_mention() {
        let (me, other) = (id(1), id(2));
        let base = Seen { author: Some(other), mentions: &[], everyone: false, reply_to: None, private: false };
        assert!(!is_mention(me, &base, false), "message ordinaire");
        assert!(is_mention(me, &Seen { mentions: &[me], ..base }, false), "@moi");
        assert!(!is_mention(me, &Seen { mentions: &[other], ..base }, false), "@quelqu'un d'autre");
        assert!(is_mention(me, &Seen { everyone: true, ..base }, false), "@everyone");
        assert!(!is_mention(me, &Seen { everyone: true, ..base }, true), "@everyone ignoré");
        assert!(is_mention(me, &Seen { everyone: true, mentions: &[me], ..base }, true), "@moi reste");
        assert!(is_mention(me, &Seen { reply_to: Some(me), ..base }, false), "réponse à mon message");
        assert!(is_mention(me, &Seen { private: true, ..base }, false), "MP");
        assert!(is_mention(me, &Seen { private: true, author: None, ..base }, false), "compte supprimé en MP");
        // Ses propres messages, jamais.
        let mine = Seen { author: Some(me), mentions: &[me], everyone: true, reply_to: Some(me), private: true };
        assert!(!is_mention(me, &mine, false));
    }

    #[test]
    fn levels_and_mutes() {
        assert_eq!(effective_level(false, None, None), Level::Mentions);
        assert_eq!(effective_level(true, None, None), Level::All);
        assert_eq!(effective_level(false, None, Some(Level::All)), Level::All);
        assert_eq!(effective_level(false, Some(Level::Nothing), Some(Level::All)), Level::Nothing);
        assert!(counts(Level::Mentions, true) && counts(Level::All, true));
        assert!(!counts(Level::Nothing, true) && !counts(Level::All, false));
        assert!(notifies(Level::All, false, false));
        assert!(!notifies(Level::Mentions, false, false) && notifies(Level::Mentions, false, true));
        assert!(!notifies(Level::Nothing, false, true));
        assert!(!notifies(Level::All, true, true), "sourdine");
        assert!(muted(Some(i64::MAX), 1_000) && muted(Some(2_000), 1_000));
        assert!(!muted(Some(1_000), 1_000) && !muted(None, 1_000));
    }
}
