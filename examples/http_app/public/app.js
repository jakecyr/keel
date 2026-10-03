document.querySelector('#send').addEventListener('click', async (event) => {
  event.target.disabled = true;
  const output = document.querySelector('#result');
  try {
    const response = await fetch('/api/echo', {method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({message: 'Hello from the browser', handledBy: 'Keel'})});
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    output.textContent = JSON.stringify(await response.json(), null, 2);
  } catch (error) { output.textContent = String(error); }
  finally { event.target.disabled = false; }
});
