// Serveur hébergé : progression du démarrage, erreurs, puis l'adresse à partager.
const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);
// Ouverte depuis le menu « Adresse à partager… » : la page reste affichée.
const sharing = location.hash === '#partager';
const titles = {
  starting: 'Démarrage de ton serveur…',
  failed: "Ton serveur n'a pas pu démarrer",
  idle: 'Aucun serveur',
};
let shown = '';

async function poll() {
  let s;
  try {
    s = await invoke('host_status');
  } catch (err) {
    s = { state: 'failed', message: String(err), details: '' };
  }
  if (s.state === 'ready' && !sharing && !s.first) {
    // Lancements suivants : on ouvre directement KRAKEN.
    try {
      await invoke('host_open');
      return;
    } catch {
      // Sinon, la page affiche l'adresse et le bouton.
    }
  }
  show(s);
  if (s.state === 'starting') setTimeout(poll, 400);
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
    $('alone').hidden = s.lan;
    $('open').textContent = s.first ? 'Créer ton compte' : 'Ouvrir KRAKEN';
  }
  $('title').textContent = s.state === 'ready' ? (s.first ? 'Ton serveur est prêt' : 'Ton serveur KRAKEN') : titles[s.state];
  if (changed && s.state !== 'starting') {
    ({ failed: $('retry'), ready: $('open'), idle: $('title') })[s.state].focus();
  }
}

$('retry').addEventListener('click', async () => {
  try {
    await invoke('host_start');
  } catch (err) {
    $('error').textContent = String(err);
    return;
  }
  poll();
});

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
