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

/// Droits qu'une surcharge de salon peut accorder ou refuser : ceux qui ont un sens dans un salon.
/// Les droits du serveur entier (rôles, bannissement, administrateur…) n'y figurent pas.
pub const CHANNEL_SCOPE: u64 = VIEW_CHANNEL
    | SEND_MESSAGES
    | SEND_IN_THREADS
    | CREATE_THREADS
    | EMBED_LINKS
    | ATTACH_FILES
    | ADD_REACTIONS
    | MENTION_EVERYONE
    | MANAGE_MESSAGES
    | READ_HISTORY
    | CONNECT
    | SPEAK
    | VIDEO
    | STREAM
    | USE_SOUNDBOARD
    | MUTE_MEMBERS
    | MOVE_MEMBERS
    | PRIORITY_SPEAKER
    | MANAGE_CHANNELS
    | CREATE_INVITE;

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

/// Rang le plus haut d'un membre : la position de son rôle le plus élevé (0 : `@everyone`
/// seulement). Le propriétaire est au-dessus de tout.
pub fn top_position(is_owner: bool, positions: impl IntoIterator<Item = i32>) -> i32 {
    if is_owner { i32::MAX } else { positions.into_iter().fold(0, i32::max) }
}

/// Hiérarchie : le propriétaire peut tout (même sur lui-même) ; les autres seulement sur
/// quelqu'un de strictement plus bas qu'eux, jamais sur le propriétaire. `ADMINISTRATOR` n'y
/// change rien.
pub fn outranks(actor_owner: bool, actor_top: i32, target_owner: bool, target_top: i32) -> bool {
    actor_owner || (!target_owner && actor_top > target_top)
}

/// Droits dans un salon, à partir de [`base`] : une seule couche de surcharges.
pub fn in_channel(base: u64, guild_id: Uuid, member_id: Uuid, member_roles: &[Uuid], overwrites: &[Overwrite]) -> u64 {
    in_layers(base, guild_id, member_id, member_roles, &[overwrites])
}

/// Droits dans un salon rangé dans une catégorie : les surcharges de la catégorie s'appliquent
/// d'abord, puis celles du salon, qui l'emportent sur elles.
pub fn in_layers(base: u64, guild_id: Uuid, member_id: Uuid, member_roles: &[Uuid], layers: &[&[Overwrite]]) -> u64 {
    if base & ADMINISTRATOR != 0 {
        return ALL;
    }
    let mut p = base;
    for overwrites in layers {
        if let Some(o) = overwrites.iter().find(|o| o.target == Target::Role(guild_id)) {
            p = (p & !o.deny) | o.allow;
        }
        let (mut allow, mut deny) = (0, 0);
        for o in *overwrites {
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
    fn a_channel_overrides_its_category() {
        let b = base(false, DEFAULT_EVERYONE, []);
        let category = [Overwrite { target: Target::Role(G), allow: 0, deny: VIEW_CHANNEL }];
        let channel = [Overwrite { target: Target::Role(R), allow: VIEW_CHANNEL, deny: 0 }];
        // Catégorie privée : personne ne voit, sauf le rôle que le salon rouvre.
        assert_eq!(in_layers(b, G, M, &[], &[&category, &[]]), 0);
        assert_eq!(in_layers(b, G, M, &[], &[&category, &channel]), 0);
        assert_ne!(in_layers(b, G, M, &[R], &[&category, &channel]), 0);
        // Les deux couches ont une surcharge de `@everyone` : celle du salon compte aussi.
        let both = [Overwrite { target: Target::Role(G), allow: 0, deny: SEND_MESSAGES }];
        let p = in_layers(b, G, M, &[], &[&[], &both]);
        assert_eq!(p & SEND_MESSAGES, 0);
        let open = [Overwrite { target: Target::Role(G), allow: SEND_MESSAGES, deny: 0 }];
        let p = in_layers(b, G, M, &[], &[&both, &open]);
        assert_ne!(p & SEND_MESSAGES, 0, "le salon reprend ce que sa catégorie retirait");
    }

    #[test]
    fn channel_scope_leaves_out_server_wide_rights() {
        assert_eq!(CHANNEL_SCOPE & !ALL, 0);
        for bit in [ADMINISTRATOR, BAN, KICK, MANAGE_ROLES, MANAGE_GUILD, TIMEOUT] {
            assert_eq!(CHANNEL_SCOPE & bit, 0);
        }
        assert_ne!(CHANNEL_SCOPE & VIEW_CHANNEL, 0);
    }

    #[test]
    fn hierarchy() {
        assert_eq!(top_position(false, []), 0);
        assert_eq!(top_position(false, [3, 7, 1]), 7);
        assert_eq!(top_position(true, [3]), i32::MAX);
        assert!(outranks(true, i32::MAX, true, i32::MAX), "le propriétaire peut tout");
        assert!(outranks(false, 5, false, 2));
        assert!(!outranks(false, 5, false, 5), "à égalité : non");
        assert!(!outranks(false, 5, false, 6));
        assert!(!outranks(false, 5, true, i32::MAX), "jamais sur le propriétaire");
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
