// Page d'accueil : rejoindre un serveur (adresse ou lien d'invitation), ou héberger le sien.
const { invoke } = window.__TAURI__.core;

const join = document.getElementById('join');
const joinError = document.getElementById('join-error');
join.addEventListener('submit', async (e) => {
  e.preventDefault();
  joinError.textContent = '';
  try {
    await invoke('join', { address: join.address.value });
  } catch (err) {
    joinError.textContent = String(err);
    join.address.focus();
  }
});

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
