//! Données de démonstration (`kraken-server --seed`) : personnages fictifs, serveurs, salons,
//! messages, amis et MP, pour que chaque écran ait quelque chose de réel à montrer.
//! Compte de démo : `demo`, mot de passe fourni par l'appelant.

use super::*;

impl Db {
    pub fn seed(&self, password_hash: &str) {
        let mut s = self.s.write().unwrap();
        let user = |s: &mut State, username: &str, display: Option<&str>| {
            let user =
                User { id: new_id(), username: username.into(), display_name: display.map(Into::into), avatar: None };
            let email = format!("{username}@demo.kraken.invalid");
            s.users.insert(user.id, UserRow { user: user.clone(), email, password_hash: password_hash.into() });
            user.id
        };
        let demo = user(&mut s, "demo", Some("Démo"));
        let lou = user(&mut s, "lou", Some("Lou"));
        let sam = user(&mut s, "sam", Some("Sam"));
        let noa = user(&mut s, "noa", None);
        let ilan = user(&mut s, "ilan", Some("Ilan"));
        let long = user(&mut s, "abyssal_navigator.42", Some("Navigatrice des grands fonds abyssaux"));

        // Abysse : le serveur principal.
        let gid = new_id();
        s.guilds.insert(gid, Guild { id: gid, name: "Abysse".into(), icon: None, owner_id: lou });
        s.roles.insert(
            gid,
            (
                gid,
                Role {
                    id: gid,
                    name: "@everyone".into(),
                    color: None,
                    position: 0,
                    permissions: perms::DEFAULT_EVERYONE,
                },
            ),
        );
        let modo = new_id();
        s.roles.insert(
            modo,
            (
                gid,
                Role {
                    id: modo,
                    name: "Modération".into(),
                    color: Some(0x2b7a78),
                    position: 1,
                    permissions: perms::MANAGE_MESSAGES | perms::KICK | perms::TIMEOUT | perms::VIEW_AUDIT_LOG,
                },
            ),
        );
        for (u, roles) in [(lou, vec![]), (demo, vec![]), (sam, vec![modo]), (noa, vec![]), (long, vec![])] {
            s.members.insert((gid, u), MemberRow { nick: None, roles, joined_at: now_ms() });
        }
        let news = s.insert_channel(Some(gid), None, ChannelKind::Announcement, "annonces");
        let open = s.insert_channel(Some(gid), None, ChannelKind::Category, "Salons ouverts");
        let general = s.insert_channel(Some(gid), Some(open.id), ChannelKind::Text, "général");
        s.channels.get_mut(&general.id).unwrap().topic = Some("On parle de tout, gentiment.".into());
        let shots = s.insert_channel(Some(gid), Some(open.id), ChannelKind::Text, "captures");
        s.insert_channel(
            Some(gid),
            Some(open.id),
            ChannelKind::Text,
            "un-salon-au-nom-vraiment-très-long-pour-tester-la-troncature",
        );
        s.insert_channel(Some(gid), Some(open.id), ChannelKind::Forum, "idées");
        let vocal = s.insert_channel(Some(gid), None, ChannelKind::Category, "Vocal");
        s.insert_channel(Some(gid), Some(vocal.id), ChannelKind::Voice, "Salle vocale");
        let team = s.insert_channel(Some(gid), None, ChannelKind::Category, "Équipe");
        let staff = s.insert_channel(Some(gid), Some(team.id), ChannelKind::Text, "staff");
        s.overwrites.insert(
            team.id,
            vec![
                Overwrite { target: Target::Role(gid), allow: 0, deny: perms::VIEW_CHANNEL },
                Overwrite { target: Target::Role(modo), allow: perms::VIEW_CHANNEL, deny: 0 },
            ],
        );
        s.insert_message(
            news.id,
            lou,
            "Bienvenue dans Abysse ! Les règles sont simples : respect et bonne humeur.",
            None,
        );
        for (a, t) in [
            (lou, "Quelqu'un a testé la nouvelle version ?"),
            (sam, "Oui, l'appel vidéo est super fluide."),
            (sam, "Et le partage d'écran en 1080p sans rien payer, c'est appréciable."),
            (noa, "Je vous rejoins ce soir dans la salle vocale."),
            (
                long,
                "Message d'un compte au nom très long, pour vérifier que l'en-tête ne déborde pas de sa colonne quand la fenêtre est étroite.",
            ),
            (lou, "Parfait, à ce soir alors."),
        ] {
            s.insert_message(general.id, a, t, None);
        }
        s.insert_message(shots.id, noa, "Ma plus belle capture de la semaine arrive bientôt.", None);
        s.insert_message(staff.id, sam, "Salon réservé à la modération.", None);

        // Récif : un second serveur, vide ou presque.
        let r = new_id();
        s.guilds.insert(r, Guild { id: r, name: "Récif".into(), icon: None, owner_id: demo });
        s.roles.insert(
            r,
            (
                r,
                Role {
                    id: r,
                    name: "@everyone".into(),
                    color: None,
                    position: 0,
                    permissions: perms::DEFAULT_EVERYONE,
                },
            ),
        );
        s.members.insert((r, demo), MemberRow { nick: None, roles: vec![], joined_at: now_ms() });
        s.insert_channel(Some(r), None, ChannelKind::Text, "accueil");

        // Social.
        for (u, k) in [
            (lou, RelationshipKind::Friend),
            (sam, RelationshipKind::Friend),
            (noa, RelationshipKind::Friend),
            (long, RelationshipKind::Friend),
            (ilan, RelationshipKind::PendingIncoming),
        ] {
            s.relationships.insert((demo, u), k);
            let back = match k {
                RelationshipKind::PendingIncoming => RelationshipKind::PendingOutgoing,
                k => k,
            };
            s.relationships.insert((u, demo), back);
        }
        let dm = s.insert_channel(None, None, ChannelKind::Dm, "");
        s.channels.get_mut(&dm.id).unwrap().name = None;
        s.recipients.insert(dm.id, vec![demo, lou]);
        s.insert_message(dm.id, lou, "Tu as vu l'invitation pour Abysse ?", None);
        let group = s.insert_channel(None, None, ChannelKind::GroupDm, "Équipée du soir");
        s.recipients.insert(group.id, vec![demo, sam, noa]);
        s.insert_message(group.id, sam, "On se retrouve à 21 h ?", None);
    }
}
