//! Permissions en bitfield 64 bits. Calcul : `@everyone` ∪ rôles, puis surcharges du salon
//! (`@everyone`, rôles, membre). Propriétaire et `ADMINISTRATOR` ont tout.

use uuid::Uuid;

macro_rules! bits {
    ($($name:ident = $bit:expr),* $(,)?) => {
        $(pub const $name: u64 = 1 << $bit;)*
        pub const ALL: u64 = 0 $(| $name)*;
    };
}

bits! {
    VIEW_CHANNEL = 0, SEND_MESSAGES = 1, SEND_IN_THREADS = 2, CREATE_THREADS = 3,
    EMBED_LINKS = 4, ATTACH_FILES = 5, ADD_REACTIONS = 6, MENTION_EVERYONE = 7,
    MANAGE_MESSAGES = 8, READ_HISTORY = 9, CONNECT = 10, SPEAK = 11, VIDEO = 12,
    STREAM = 13, USE_SOUNDBOARD = 14, MUTE_MEMBERS = 15, MOVE_MEMBERS = 16,
    PRIORITY_SPEAKER = 17, MANAGE_CHANNELS = 18, MANAGE_ROLES = 19, MANAGE_EMOJIS = 20,
    MANAGE_EVENTS = 21, KICK = 22, BAN = 23, TIMEOUT = 24, MANAGE_GUILD = 25,
    VIEW_AUDIT_LOG = 26, CREATE_INVITE = 27, CHANGE_NICK = 28, MANAGE_NICKS = 29,
    ADMINISTRATOR = 30,
}

/// Droits de `@everyone` dans un nouveau serveur.
pub const DEFAULT_EVERYONE: u64 = VIEW_CHANNEL
    | SEND_MESSAGES
    | SEND_IN_THREADS
    | CREATE_THREADS
    | EMBED_LINKS
    | ATTACH_FILES
    | ADD_REACTIONS
    | READ_HISTORY
    | CONNECT
    | SPEAK
    | VIDEO
    | STREAM
    | USE_SOUNDBOARD
    | CREATE_INVITE
    | CHANGE_NICK;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Role(Uuid),
    Member(Uuid),
}

#[derive(Clone, Copy, Debug)]
pub struct Overwrite {
    pub target: Target,
    pub allow: u64,
    pub deny: u64,
}

/// Droits au niveau du serveur. `everyone` est le rôle dont l'id vaut celui du serveur.
pub fn base(is_owner: bool, everyone: u64, member_roles: impl IntoIterator<Item = u64>) -> u64 {
    if is_owner {
        return ALL;
    }
    let p = member_roles.into_iter().fold(everyone, |acc, r| acc | r);
    if p & ADMINISTRATOR != 0 { ALL } else { p }
}

/// Droits dans un salon, à partir de [`base`].
pub fn in_channel(base: u64, guild_id: Uuid, member_id: Uuid, member_roles: &[Uuid], overwrites: &[Overwrite]) -> u64 {
    if base & ADMINISTRATOR != 0 {
        return ALL;
    }
    let mut p = base;
    if let Some(o) = overwrites.iter().find(|o| o.target == Target::Role(guild_id)) {
        p = (p & !o.deny) | o.allow;
    }
    let (mut allow, mut deny) = (0, 0);
    for o in overwrites {
        if let Target::Role(r) = o.target
            && r != guild_id
            && member_roles.contains(&r)
        {
            allow |= o.allow;
            deny |= o.deny;
        }
    }
    p = (p & !deny) | allow;
    if let Some(o) = overwrites.iter().find(|o| o.target == Target::Member(member_id)) {
        p = (p & !o.deny) | o.allow;
    }
    // Sans VIEW_CHANNEL, rien d'autre ne compte.
    if p & VIEW_CHANNEL == 0 { 0 } else { p }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: Uuid = Uuid::from_bytes([1; 16]);
    const R: Uuid = Uuid::from_bytes([2; 16]);
    const M: Uuid = Uuid::from_bytes([3; 16]);

    #[test]
    fn owner_and_admin_have_everything() {
        assert_eq!(base(true, 0, []), ALL);
        assert_eq!(base(false, 0, [ADMINISTRATOR]), ALL);
    }

    #[test]
    fn overwrites_apply_in_order() {
        let b = base(false, DEFAULT_EVERYONE, []);
        let ow = [
            Overwrite { target: Target::Role(G), allow: 0, deny: SEND_MESSAGES },
            Overwrite { target: Target::Role(R), allow: SEND_MESSAGES, deny: 0 },
        ];
        assert_eq!(in_channel(b, G, M, &[], &ow) & SEND_MESSAGES, 0);
        assert_ne!(in_channel(b, G, M, &[R], &ow) & SEND_MESSAGES, 0);
        let ow2 = [ow[0], ow[1], Overwrite { target: Target::Member(M), allow: 0, deny: VIEW_CHANNEL }];
        assert_eq!(in_channel(b, G, M, &[R], &ow2), 0);
    }
}
