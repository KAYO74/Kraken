# Vocal natif de l'appli de bureau Linux

**Pourquoi.** Le navigateur intégré de Linux (WebKitGTK de Debian, Ubuntu, Fedora) est compilé sans WebRTC : aucun réglage n'y
change rien (voir `apps/desktop/src/media.rs`). Windows et macOS ont WebRTC dans leur webview et ne changent pas. Fred a choisi un
client vocal natif en Rust (2026-11-23) plutôt qu'un navigateur embarqué.

**Principe.** Le client natif est un « navigateur » de plus pour le SFU (`kraken-voice`, str0m) : même signalisation, aucun
changement du serveur. Il fait l'offre avec les quatre transceivers `sendonly` (micro, image de l'écran, son de l'écran, caméra,
dans cet ordre) et le canal de données `kraken`, puis répond aux offres du serveur par ce canal, comme le client web
(`kraken-proto` : `VoiceJoin`, `VoiceClientMsg`, `VoiceServerMsg`). Modèle de départ : le pair de test de
`crates/kraken-voice/tests/roundtrip.rs`.

**Ce que le natif fait (voix seulement).** Micro (cpal, ALSA, donc aussi PulseAudio et PipeWire), Opus 48 kHz mono 20 ms, réception de
chaque participant (décodage, mélange, haut-parleur), micro coupé, sourdine, indicateurs « parle », niveaux, choix des périphériques,
gain. **Ce qu'il ne fait pas** : partage d'écran et caméra (ni envoi, ni réception : il faudrait décoder du VP8/H264 et l'afficher dans la
page), annulation d'écho (casque conseillé), contrôle à distance.

**Étapes (une PR chacune).**
1. **Fait.** `crates/kraken-voice-native` : la bibliothèque, sans Tauri ni interface. Moteur str0m sur son fil (UDP, ICE, DTLS, SRTP, canal de
   données), Opus, rééchantillonnage, mélange, gigue, périphériques cpal derrière un trait (`Capture`, `Playback`) avec une version
   synthétique pour les tests. Tests de bout en bout contre le vrai SFU en mémoire, sans carte son. Aucun changement visible.
2. L'appli de bureau (Linux seulement) : commandes Tauri (`voice_native_*`, gardées comme `open_in_browser`), le client web les
   utilise quand le navigateur n'a pas WebRTC et que l'appli de bureau est là ; états relevés par la page (« poll ») ; Réglages › Voix et
   vidéo avec les périphériques natifs ; paquets (`libasound2`) ; version 0.27.0.
3. Finitions selon les retours : réduction de bruit (nnnoiseless), appui pour parler, niveaux, message clair sur ce qui manque.

**Décisions.** La page fait l'appel HTTP (`POST /channels/{id}/voice`, elle a la session) : le natif fabrique l'offre, la page la
poste, puis rend la réponse au natif. Pas de chiffrement de bout en bout aujourd'hui (rien à faire). Opus lié statiquement si
possible (pas de dépendance à `libopus` à l'exécution).
