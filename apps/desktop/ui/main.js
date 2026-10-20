// Page d'accueil : rejoindre un serveur (adresse ou lien d'invitation), ou héberger le sien.
const { invoke } = window.__TAURI__.core;

const join = document.getElementById('join');
const joinError = document.getElementById('join-error');
const joinSubmit = document.getElementById('join-submit');
join.addEventListener('submit', async (e) => {
  e.preventDefault();
  joinError.textContent = '';
  joinSubmit.disabled = true;
  try {
    await invoke('join', { address: join.address.value });
  } catch (err) {
    joinError.textContent = String(err);
    join.address.focus();
  } finally {
    joinSubmit.disabled = false;
  }
});

// Au lancement, le serveur retenu ne répondait pas : son adresse, prête à réessayer.
const lost = new URLSearchParams(location.hash.slice(1)).get('injoignable');
if (lost) {
  join.address.value = lost;
  joinError.textContent = "Ce serveur ne répond pas. Vérifie que l'ordinateur qui l'héberge a KRAKEN ouvert, puis clique sur « Rejoindre ».";
}

// Prévenir avant d'arrêter le serveur de cet ordinateur.
invoke('host_status').then((s) => {
  document.getElementById('hosting').hidden = s.state !== 'ready' && s.state !== 'starting';
}, () => {});

const host = document.getElementById('host');
const hostError = document.getElementById('host-error');
host.addEventListener('click', async () => {
  hostError.textContent = '';
  host.disabled = true;
  try {
    await invoke('host_start');
    location.href = 'host.html';
  } catch (err) {
    hostError.textContent = String(err);
    host.disabled = false;
    host.focus();
  }
});
