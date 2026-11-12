// Fichier séparé : la CSP du serveur interdit les scripts en ligne.
//
// Script classique, chargé dans <head> : il bloque le premier rendu le temps de poser les réglages
// d'affichage de cet appareil (Apparence, Accessibilité) sur <html>, pour que la page ne clignote pas
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

  set("data-theme", read("kraken.theme", ["auto", "dark", "light", "black"], "dark"));
  var text = read("kraken.text", ["14", "16", "18", "20"], "16");
  set("data-text", text === "16" ? null : text);
  var zoom = read("kraken.zoom", ["90", "100", "110", "125", "150"], "100");
  set("data-zoom", zoom === "100" ? null : zoom);
  set("data-density", read("kraken.density", ["comfortable", "compact"], "comfortable") === "compact" ? "compact" : null);
  var motion = read("kraken.motion", ["auto", "reduce", "normal"], "auto");
  set("data-motion", motion === "reduce" || (motion === "auto" && media("(prefers-reduced-motion: reduce)")) ? "reduce" : "normal");
  var contrast = read("kraken.contrast", ["normal", "high", "auto"], "normal");
  set("data-contrast", contrast === "high" || (contrast === "auto" && media("(prefers-contrast: more)")) ? "high" : null);
  set("data-underline-links", read("kraken.underline", ["on", "off"], "off") === "on" ? "" : null);
})();

import("/kraken-client.js").then(function (client) { return client.default(); });
