// Fichier séparé : la CSP du serveur interdit les scripts en ligne.
//
// Script classique, chargé dans <head> : il bloque le premier rendu le temps de poser les réglages
// d'affichage de cet appareil (Apparence, Accessibilité) et la langue sur <html>, pour que la page ne clignote pas
// avant que le client WASM ne démarre. Mêmes clés et mêmes attributs que src/prefs.rs ; sans stockage
// local (navigation privée, accès refusé) les valeurs par défaut restent en place.
(function () {
  var root = document.documentElement;
  var store = null;
  try { store = window.localStorage; } catch (e) { store = null; }
  function read(key, allowed, fallback) {
    try {
      var v = store && store.getItem(key);
      if (allowed.indexOf(v) >= 0) return v;
    } catch (e) { /* stockage refusé */ }
    return fallback;
  }
  function media(query) {
    try { return window.matchMedia(query).matches; } catch (e) { return false; }
  }
  function set(name, value) {
    if (value === null) root.removeAttribute(name); else root.setAttribute(name, value);
  }

  // Vieux WebKit : WebRTC sous son nom préfixé seulement. Sans cet alias le client croit les appels vocaux absents.
  if (!window.RTCPeerConnection && window.webkitRTCPeerConnection) window.RTCPeerConnection = window.webkitRTCPeerConnection;

  // Langue de l'interface (même choix et même règle que src/i18n.rs) : `<html lang dir>` avant le client WASM.
  var chosen = read("kraken.lang", ["auto", "fr", "en"], "auto");
  try { if (chosen === "auto") chosen = window.sessionStorage.getItem("kraken.lang") || "auto"; } catch (e) { /* stockage refusé */ }
  var lang = chosen === "fr" || chosen === "en" ? chosen : "fr";
  if (chosen === "auto") {
    var wanted = navigator.languages && navigator.languages.length ? navigator.languages : [navigator.language];
    for (var i = 0; i < wanted.length; i++) {
      var code = String(wanted[i] || "").split(/[-_]/)[0].toLowerCase();
      if (code === "fr" || code === "en") { lang = code; break; }
    }
  }
  root.setAttribute("lang", lang);
  // Sens d'écriture : la même table que `Lang::dir` (kraken-core). Aucune langue de droite à gauche n'est livrée encore (`lang` vaut
  // fr ou en) : la table est prête pour l'arabe, le persan et l'hébreu, que la feuille de style et le client suivent en direct.
  var rtl = ["ar", "fa", "he"];
  root.setAttribute("dir", rtl.indexOf(lang) >= 0 ? "rtl" : "ltr");

  set("data-theme", read("kraken.theme", ["auto", "dark", "light", "black"], "dark"));
  var text = read("kraken.text", ["14", "16", "18", "20"], "16");
  set("data-text", text === "16" ? null : text);
  var zoom = read("kraken.zoom", ["90", "100", "110", "125", "150"], "100");
  set("data-zoom", zoom === "100" ? null : zoom);
  // Zoom plafonné à la largeur de la fenêtre (même calcul que prefs.rs) : les points de rupture des
  // feuilles de style lisent la largeur réelle, donc la page zoomée doit rester au moins aussi large
  // que le point de rupture où l'on se trouve (1024 px, puis 640 px), et le zoom vaut 1 sous 640 px.
  var width = window.innerWidth, choice = Number(zoom) / 100;
  var applied = width < 640 ? 1 : Math.min(choice, width / (width >= 1024 ? 1024 : 640));
  if (Math.abs(applied - choice) > 1e-6) root.style.setProperty("--zoom", String(Math.round(applied * 1000) / 1000));
  set("data-density", read("kraken.density", ["comfortable", "compact"], "comfortable") === "compact" ? "compact" : null);
  var motion = read("kraken.motion", ["auto", "reduce", "normal"], "auto");
  set("data-motion", motion === "reduce" || (motion === "auto" && media("(prefers-reduced-motion: reduce)")) ? "reduce" : "normal");
  var contrast = read("kraken.contrast", ["normal", "high", "auto"], "normal");
  set("data-contrast", contrast === "high" || (contrast === "auto" && media("(prefers-contrast: more)")) ? "high" : null);
  set("data-underline-links", read("kraken.underline", ["on", "off"], "off") === "on" ? "" : null);
})();

import("/kraken-client.js").then(function (client) { return client.default(); });
