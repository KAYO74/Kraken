// Envoie l'adresse saisie au côté Rust, qui la retient et ouvre l'instance.
const form = document.querySelector('form');
const error = document.getElementById('error');
form.addEventListener('submit', async (e) => {
  e.preventDefault();
  error.textContent = '';
  try {
    await window.__TAURI__.core.invoke('open_instance', { address: form.address.value });
  } catch (err) {
    error.textContent = String(err);
  }
});
