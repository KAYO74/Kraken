// Serveur hébergé : progression du démarrage, erreurs, puis l'adresse à partager.
const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);
// Ouverte depuis le menu « Adresse à partager… » : la page reste affichée.
const sharing = location.hash === '#partager';
// Sinon, KRAKEN s'ouvre directement dès qu'il est prêt, sauf s'il y a quelque chose à lire.
let auto = !sharing;
const titles = {
  starting: 'Démarrage de ton serveur…',
  failed: "Ton serveur n'a pas pu démarrer",
  idle: 'Aucun serveur',
};
let shown = '';
let timer = 0;

// Un seul suivi à la fois, même après « Réessayer ».
function later(ms) {
  clearTimeout(timer);
  timer = setTimeout(poll, ms);
}

async function poll() {
  let s;
  try {
    s = await invoke('host_status');
  } catch (err) {
    s = { state: 'failed', message: String(err), details: '' };
  }
  if (s.state === 'ready' && auto && !s.first && s.lan && !s.fresh && !s.stale) {
    try {
      await invoke('host_open');
      return;
    } catch {
      // Sinon, la page affiche l'adresse et le bouton.
    }
  }
  if (s.state !== 'starting') auto = false;
  show(s);
  // Prêt : on suit le premier compte et les changements de réseau.
  if (s.state === 'starting') later(400);
  else if (s.state === 'ready') later(3000);
}

function show(s) {
  for (const id of ['starting', 'failed', 'ready', 'idle']) $(id).hidden = s.state !== id;
  const changed = shown !== s.state;
  shown = s.state;
  if (s.state === 'starting') {
    $('step').textContent = s.step;
  } else if (s.state === 'failed') {
    $('error').textContent = s.message;
    $('details').textContent = s.details;
    $('more').hidden = !s.details;
  } else if (s.state === 'ready') {
    $('address').value = s.url;
    $('alone').hidden = s.lan || s.stale;
    $('fresh').hidden = !s.fresh || s.first;
    $('stale').hidden = !s.stale;
    $('open').textContent = s.first ? 'Créer ton compte' : 'Ouvrir KRAKEN';
  }
  $('title').textContent = s.state === 'ready' ? (s.first ? 'Ton serveur est prêt' : 'Ton serveur KRAKEN') : titles[s.state];
  if (changed && s.state !== 'starting') {
    ({ failed: $('retry'), ready: $('open'), idle: $('title') })[s.state].focus();
  }
}

async function restart(errorId) {
  try {
    await invoke('host_start');
  } catch (err) {
    $(errorId).textContent = String(err);
    return;
  }
  auto = !sharing;
  poll();
}

$('retry').addEventListener('click', () => restart('error'));
$('restart').addEventListener('click', () => restart('copied'));

$('open').addEventListener('click', async () => {
  try {
    await invoke('host_open');
  } catch (err) {
    $('copied').textContent = String(err);
  }
});

$('copy').addEventListener('click', async () => {
  const address = $('address');
  try {
    await navigator.clipboard.writeText(address.value);
  } catch {
    address.select();
    if (!document.execCommand('copy')) {
      $('copied').textContent = "Sélectionne l'adresse et copie-la.";
      return;
    }
  }
  $('copied').textContent = 'Adresse copiée.';
});

$('address').addEventListener('focus', (e) => e.target.select());

poll();
