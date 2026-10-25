//! La boucle UDP : une prise pour tout le monde, chaque paquet remis au `Rtc` qui le reconnaît,
//! les trames relayées aux autres membres du salon, les renégociations envoyées par le canal de
//! données. Tout est sur ce fil : pas de verrou, sauf pour les états vocaux partagés.
//!
//! Un message du canal de données qui concerne d'autres participants devient un [`Item`], traité
//! par [`Sfu::apply`] qui voit tout le salon : c'est là qu'un nouveau message (le contrôle à
//! distance, par exemple) trouvera son destinataire, puis [`Client::send`] le lui remettra.

use std::collections::HashSet;
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use kraken_proto::{Id, VOICE_DATA_CHANNEL, VoiceClientMsg, VoiceServerMsg};
use str0m::change::{SdpAnswer, SdpPendingOffer};
use str0m::channel::{ChannelData, ChannelId};
use str0m::media::{Direction, KeyframeRequestKind, MediaData, Mid};
use str0m::net::{Protocol, Receive};
use str0m::{Event, IceConnectionState, Input, Output, Rtc};

use crate::{Cmd, Join, Kind, Shared};

/// Connexion (ICE, DTLS et canal de données) à établir dans ce délai.
const SETUP: Duration = Duration::from_secs(15);
/// ICE coupé plus longtemps : la connexion est perdue.
const LOST: Duration = Duration::from_secs(10);
/// Attente maximale sur la prise : les ordres de l'API sont lus au moins à ce rythme.
const TICK: Duration = Duration::from_millis(20);
/// Au plus une demande d'image clé par écran dans ce délai.
const KEYFRAME_GAP: Duration = Duration::from_millis(500);
/// Message du canal de données plus long : ignoré.
const MAX_MESSAGE: usize = 256 * 1024;

/// Une piste relayée vers ce client : à qui elle est, et où en est sa négociation.
struct Out {
    from: Id,
    kind: Kind,
    state: OutState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutState {
    ToOpen,
    Negotiating(Mid),
    Open(Mid),
    ToStop(Mid),
    NegotiatingStop(Mid),
}

/// Un participant.
pub(crate) struct Client {
    session: u64,
    user: Id,
    room: Id,
    rtc: Rtc,
    /// Les mids de ce qu'il envoie, dans l'ordre de [`Kind::ALL`].
    sources: [Mid; 3],
    can_speak: bool,
    can_stream: bool,
    /// Micro coupé (ou son coupé) : ce qu'il enverrait quand même n'est pas relayé.
    muted: bool,
    self_deaf: bool,
    streaming: bool,
    /// Ceux dont il regarde l'écran.
    watching: HashSet<Id>,
    channel: Option<ChannelId>,
    pending: Option<SdpPendingOffer>,
    outs: Vec<Out>,
    created: Instant,
    connected: bool,
    lost_since: Option<Instant>,
    /// Prochaine échéance de son `Rtc`.
    next: Instant,
    /// À interroger (`poll_output`) : il a reçu quelque chose.
    dirty: bool,
    /// Pistes à renégocier dès que possible.
    stale: bool,
    last_keyframe: Option<Instant>,
}

/// Ce qu'un client produit et qui concerne les autres.
enum Item {
    Media { from: Id, room: Id, kind: Kind, data: Box<MediaData> },
    Keyframe { user: Id, kind: KeyframeRequestKind },
    State { session: u64, self_mute: bool, self_deaf: bool, streaming: bool },
    Watch { session: u64, user: Id, on: bool },
}

impl Client {
    pub(crate) fn new(rtc: Rtc, now: Instant, session: u64, j: &Join, sources: [Mid; 3]) -> Self {
        Self {
            session,
            user: j.user_id,
            room: j.channel_id,
            rtc,
            sources,
            can_speak: j.can_speak,
            can_stream: j.can_stream,
            muted: j.self_mute || j.self_deaf,
            self_deaf: j.self_deaf,
            streaming: false,
            watching: HashSet::new(),
            channel: None,
            pending: None,
            outs: Vec::new(),
            created: now,
            connected: false,
            lost_since: None,
            next: now,
            dirty: true,
            stale: true,
            last_keyframe: None,
        }
    }

    fn kind_of(&self, mid: Mid) -> Option<Kind> {
        self.sources.iter().position(|m| *m == mid).map(|i| Kind::ALL[i])
    }

    fn input(&mut self, input: Input) {
        if self.rtc.is_alive()
            && let Err(e) = self.rtc.handle_input(input)
        {
            tracing::debug!("vocal {} : entrée refusée ({e})", self.user);
            self.rtc.disconnect();
        }
        self.dirty = true;
    }

    /// Vide la sortie du `Rtc` : paquets envoyés, événements traités, prochaine échéance notée.
    fn poll(&mut self, socket: &UdpSocket, items: &mut Vec<Item>) {
        while self.rtc.is_alive() {
            match self.rtc.poll_output() {
                Ok(Output::Timeout(t)) => {
                    self.next = t;
                    return;
                }
                Ok(Output::Transmit(t)) => {
                    // Une erreur d'envoi (réseau coupé…) se voit ensuite par ICE.
                    let _ = socket.send_to(&t.contents, t.destination);
                }
                Ok(Output::Event(e)) => self.event(e, items),
                Err(e) => {
                    tracing::debug!("vocal {} : {e}", self.user);
                    self.rtc.disconnect();
                }
            }
        }
    }

    fn event(&mut self, e: Event, items: &mut Vec<Item>) {
        match e {
            Event::Connected => self.connected = true,
            Event::IceConnectionStateChange(IceConnectionState::Disconnected) => {
                self.lost_since.get_or_insert_with(Instant::now);
            }
            Event::IceConnectionStateChange(IceConnectionState::Connected | IceConnectionState::Completed) => {
                self.lost_since = None;
            }
            Event::ChannelOpen(id, label) if label == VOICE_DATA_CHANNEL && self.channel.is_none() => {
                self.channel = Some(id);
                self.stale = true;
            }
            Event::ChannelClose(id) if Some(id) == self.channel => {
                tracing::debug!("vocal {} : canal de données fermé", self.user);
                self.rtc.disconnect();
            }
            Event::ChannelData(d) if Some(d.id) == self.channel => self.message(d, items),
            Event::MediaData(data) => {
                let Some(kind) = self.kind_of(data.mid) else { return };
                let allowed = match kind {
                    Kind::Mic => self.can_speak && !self.muted,
                    Kind::Screen | Kind::ScreenAudio => self.can_stream && self.streaming,
                };
                if !allowed {
                    return;
                }
                if kind == Kind::Screen && !data.contiguous {
                    self.request_keyframe(KeyframeRequestKind::Pli);
                }
                items.push(Item::Media { from: self.user, room: self.room, kind, data: Box::new(data) });
            }
            // Un spectateur demande une image clé : on la demande à celui qui partage.
            Event::KeyframeRequest(req) => {
                let from = self.outs.iter().find(|o| o.state == OutState::Open(req.mid)).map(|o| o.from);
                if let Some(user) = from {
                    items.push(Item::Keyframe { user, kind: req.kind });
                }
            }
            _ => {}
        }
    }

    fn message(&mut self, d: ChannelData, items: &mut Vec<Item>) {
        if d.data.len() > MAX_MESSAGE {
            return;
        }
        let Ok(msg) = serde_json::from_slice::<VoiceClientMsg>(&d.data) else { return };
        match msg {
            VoiceClientMsg::Answer { sdp } => self.answer(&sdp, items),
            VoiceClientMsg::State { self_mute, self_deaf, streaming } => {
                items.push(Item::State { session: self.session, self_mute, self_deaf, streaming })
            }
            VoiceClientMsg::Watch { user_id, on } => {
                items.push(Item::Watch { session: self.session, user: user_id, on })
            }
        }
    }

    fn answer(&mut self, sdp: &str, items: &mut Vec<Item>) {
        let Some(pending) = self.pending.take() else { return };
        let accepted = SdpAnswer::from_sdp_string(sdp)
            .map_err(|e| e.to_string())
            .and_then(|a| self.rtc.sdp_api().accept_answer(pending, a).map_err(|e| e.to_string()));
        if let Err(e) = accepted {
            tracing::debug!("vocal {} : réponse refusée ({e})", self.user);
            self.rtc.disconnect();
            return;
        }
        for o in &mut self.outs {
            if let OutState::Negotiating(m) = o.state {
                o.state = OutState::Open(m);
                // Un nouveau spectateur : il lui faut une image clé pour commencer.
                if o.kind == Kind::Screen {
                    items.push(Item::Keyframe { user: o.from, kind: KeyframeRequestKind::Pli });
                }
            }
        }
        // Les lignes arrêtées pourront resservir à la prochaine piste.
        self.outs.retain(|o| !matches!(o.state, OutState::NegotiatingStop(_)));
        self.stale = true;
    }

    fn request_keyframe(&mut self, kind: KeyframeRequestKind) {
        if self.last_keyframe.is_some_and(|t| t.elapsed() < KEYFRAME_GAP) {
            return;
        }
        let mid = self.sources[1];
        if let Some(mut w) = self.rtc.writer(mid) {
            // Échoue tant que rien n'a été reçu sur cette piste : sans gravité.
            let _ = w.request_keyframe(None, kind);
            self.last_keyframe = Some(Instant::now());
            self.dirty = true;
        }
    }

    /// Ajuste les pistes relayées à `wanted` ; une seule offre à la fois, par le canal de données.
    fn negotiate(&mut self, wanted: &HashSet<(Id, Kind)>) {
        let Some(channel) = self.channel else { return };
        if self.pending.is_some() || !self.rtc.is_alive() {
            return;
        }
        self.stale = false;
        self.outs.retain(|o| o.state != OutState::ToOpen || wanted.contains(&(o.from, o.kind)));
        for o in &mut self.outs {
            if let OutState::Open(m) = o.state
                && !wanted.contains(&(o.from, o.kind))
            {
                o.state = OutState::ToStop(m);
            }
        }
        for &(from, kind) in wanted {
            let live = self.outs.iter().any(|o| {
                o.from == from
                    && o.kind == kind
                    && matches!(o.state, OutState::ToOpen | OutState::Negotiating(_) | OutState::Open(_))
            });
            if !live {
                self.outs.push(Out { from, kind, state: OutState::ToOpen });
            }
        }
        let mut api = self.rtc.sdp_api();
        for o in &mut self.outs {
            match o.state {
                OutState::ToOpen => {
                    let (stream, track) = (Some(o.kind.stream_id(o.from)), Some(o.kind.track_id(o.from)));
                    let mid = api.add_media(o.kind.media(), Direction::SendOnly, stream, track, None);
                    o.state = OutState::Negotiating(mid);
                }
                OutState::ToStop(m) => {
                    api.stop_media(m);
                    o.state = OutState::NegotiatingStop(m);
                }
                _ => {}
            }
        }
        if !api.has_changes() {
            return;
        }
        let Some((offer, pending)) = api.apply() else { return };
        if self.send(channel, &VoiceServerMsg::Offer { sdp: offer.to_sdp_string() }) {
            self.pending = Some(pending);
        }
    }

    /// Un message au navigateur par le canal de données (offres ; demain, d'autres messages
    /// relayés entre participants). Échec : la connexion est coupée.
    fn send(&mut self, channel: ChannelId, msg: &VoiceServerMsg) -> bool {
        let json = serde_json::to_string(msg).expect("json");
        let sent = matches!(self.rtc.channel(channel).map(|mut c| c.write(false, json.as_bytes())), Some(Ok(_)));
        if sent {
            self.dirty = true;
        } else {
            tracing::debug!("vocal {} : message non envoyé", self.user);
            self.rtc.disconnect();
        }
        sent
    }

    /// Relaie une trame de `from` si ce client reçoit cette piste.
    fn forward(&mut self, from: Id, kind: Kind, data: &MediaData) {
        if kind.is_audio() && self.self_deaf {
            return;
        }
        let Some(mid) = self.outs.iter().find_map(|o| match o.state {
            OutState::Open(m) if o.from == from && o.kind == kind => Some(m),
            _ => None,
        }) else {
            return;
        };
        let Some(writer) = self.rtc.writer(mid) else { return };
        let Some(pt) = writer.match_params(data.params) else { return };
        if let Err(e) = writer.write(pt, data.network_time, data.time, data.data.clone()) {
            tracing::debug!("vocal {} : trame non relayée ({e})", self.user);
        }
        self.dirty = true;
    }

    /// Pourquoi ce client doit partir, s'il le doit.
    fn gone(&self, now: Instant) -> Option<&'static str> {
        if !self.rtc.is_alive() {
            Some("connexion fermée")
        } else if !(self.connected && self.channel.is_some()) && now.duration_since(self.created) > SETUP {
            Some("connexion jamais établie")
        } else if self.lost_since.is_some_and(|t| now.duration_since(t) > LOST) {
            Some("connexion perdue")
        } else {
            None
        }
    }
}

/// Tous les clients, sur ce fil.
struct Sfu {
    socket: UdpSocket,
    local: SocketAddr,
    shared: Arc<Shared>,
    clients: Vec<Client>,
    items: Vec<Item>,
}

/// Si la boucle s'arrête (ou panique), plus personne n'est en vocal.
struct Cleanup(Arc<Shared>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let users: Vec<Id> = self.0.lock().keys().copied().collect();
        for u in users {
            self.0.release(u, None);
        }
    }
}

pub(crate) fn run(socket: UdpSocket, local: SocketAddr, rx: Receiver<Cmd>, shared: Arc<Shared>) {
    let _cleanup = Cleanup(shared.clone());
    let mut sfu = Sfu { socket, local, shared, clients: Vec::new(), items: Vec::new() };
    let mut buf = vec![0u8; 2000];
    loop {
        loop {
            match rx.try_recv() {
                Ok(cmd) => sfu.command(cmd),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let now = Instant::now();
        sfu.sweep(now);
        for c in &mut sfu.clients {
            if c.next <= now {
                c.input(Input::Timeout(now));
            }
        }
        sfu.drain();
        sfu.receive(&mut buf);
    }
}

impl Sfu {
    fn command(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Join(c) => {
                // Une connexion par utilisateur : l'ancienne est remplacée (son état l'est déjà).
                if let Some(i) = self.clients.iter().position(|o| o.user == c.user) {
                    self.remove(i, "remplacée");
                }
                tracing::debug!("vocal {} : connexion au salon {}", c.user, c.room);
                self.touch(c.room);
                self.clients.push(*c);
            }
            Cmd::Leave { session } => {
                if let Some(i) = self.clients.iter().position(|c| c.session == session) {
                    self.remove(i, "départ");
                }
            }
        }
    }

    fn sweep(&mut self, now: Instant) {
        let mut i = 0;
        while i < self.clients.len() {
            match self.clients[i].gone(now) {
                Some(why) => self.remove(i, why),
                None => i += 1,
            }
        }
    }

    fn remove(&mut self, i: usize, why: &str) {
        let mut c = self.clients.swap_remove(i);
        c.rtc.disconnect();
        tracing::debug!("vocal {} : fin ({why})", c.user);
        self.shared.release(c.user, Some(c.session));
        for o in self.clients.iter_mut().filter(|o| o.room == c.room) {
            o.watching.remove(&c.user);
        }
        self.touch(c.room);
    }

    /// Le salon a changé : chacun doit revoir ses pistes.
    fn touch(&mut self, room: Id) {
        for c in self.clients.iter_mut().filter(|c| c.room == room) {
            c.stale = true;
        }
    }

    /// Renégocie, interroge et relaie jusqu'à ce qu'il n'y ait plus rien à faire.
    fn drain(&mut self) {
        loop {
            self.renegotiate();
            let mut busy = false;
            for c in self.clients.iter_mut().filter(|c| c.dirty) {
                busy = true;
                c.dirty = false;
                c.poll(&self.socket, &mut self.items);
            }
            let items = std::mem::take(&mut self.items);
            if items.is_empty() && !busy {
                return;
            }
            for item in items {
                self.apply(item);
            }
        }
    }

    /// Ce que chacun doit recevoir : le micro des autres (s'ils ont le droit de parler), et
    /// l'écran de ceux qu'il regarde (s'ils partagent et en ont le droit).
    fn renegotiate(&mut self) {
        if !self.clients.iter().any(|c| c.stale && c.channel.is_some() && c.pending.is_none()) {
            return;
        }
        let members: Vec<(Id, Id, bool, bool)> =
            self.clients.iter().map(|c| (c.room, c.user, c.can_speak, c.can_stream && c.streaming)).collect();
        for c in self.clients.iter_mut().filter(|c| c.stale) {
            let mut wanted = HashSet::new();
            for &(room, user, speaks, streams) in &members {
                if room != c.room || user == c.user {
                    continue;
                }
                if speaks {
                    wanted.insert((user, Kind::Mic));
                }
                if streams && c.watching.contains(&user) {
                    wanted.insert((user, Kind::Screen));
                    wanted.insert((user, Kind::ScreenAudio));
                }
            }
            c.negotiate(&wanted);
        }
    }

    fn apply(&mut self, item: Item) {
        match item {
            Item::Media { from, room, kind, data } => {
                for c in self.clients.iter_mut().filter(|c| c.room == room && c.user != from) {
                    c.forward(from, kind, &data);
                }
            }
            Item::Keyframe { user, kind } => {
                if let Some(c) = self.clients.iter_mut().find(|c| c.user == user && c.streaming) {
                    c.request_keyframe(kind);
                }
            }
            Item::State { session, self_mute, self_deaf, streaming } => {
                let Some(c) = self.clients.iter_mut().find(|c| c.session == session) else { return };
                let streaming = streaming && c.can_stream;
                let stopped = c.streaming && !streaming;
                let changed = c.streaming != streaming;
                c.streaming = streaming;
                c.muted = self_mute || self_deaf;
                c.self_deaf = self_deaf;
                let (user, room) = (c.user, c.room);
                self.shared.update(user, session, |s| {
                    s.self_mute = self_mute || self_deaf;
                    s.self_deaf = self_deaf;
                    s.streaming = streaming;
                });
                // Partage arrêté : plus personne ne le regarde ; reprendre demande un nouveau `watch`.
                if stopped {
                    for o in self.clients.iter_mut().filter(|o| o.room == room) {
                        o.watching.remove(&user);
                    }
                }
                if changed {
                    self.touch(room);
                }
            }
            Item::Watch { session, user, on } => {
                let Some(me) = self.clients.iter().find(|c| c.session == session) else { return };
                let (me_user, room) = (me.user, me.room);
                let streams = self
                    .clients
                    .iter()
                    .any(|c| c.user == user && c.room == room && c.streaming && c.can_stream && user != me_user);
                let Some(me) = self.clients.iter_mut().find(|c| c.session == session) else { return };
                let changed = if on && streams { me.watching.insert(user) } else { me.watching.remove(&user) };
                if changed {
                    me.stale = true;
                }
            }
        }
    }

    /// Attend un paquet (au plus jusqu'à la prochaine échéance) et le remet à son client.
    fn receive(&mut self, buf: &mut [u8]) {
        let now = Instant::now();
        let next = self.clients.iter().map(|c| c.next).min().unwrap_or(now + TICK).min(now + TICK);
        let wait = next.saturating_duration_since(now).max(Duration::from_millis(1));
        if self.socket.set_read_timeout(Some(wait)).is_err() {
            return;
        }
        let (n, source) = match self.socket.recv_from(buf) {
            Ok(r) => r,
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => return,
            // Windows signale ainsi un ancien envoi refusé (ICMP) ; ailleurs, rien de durable.
            Err(e) => {
                tracing::trace!("vocal : lecture UDP ({e})");
                return;
            }
        };
        let Ok(rx) = Receive::new(Protocol::Udp, source, self.local, &buf[..n]) else { return };
        let input = Input::Receive(Instant::now(), rx);
        if let Some(c) = self.clients.iter_mut().find(|c| c.rtc.accepts(&input)) {
            c.input(input);
        }
    }
}
