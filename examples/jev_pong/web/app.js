'use strict';
const $ = id => document.getElementById(id);
const canvas = $('court'), ctx = canvas.getContext('2d');
const names = ['READY', 'OFFLINE', 'JEV', 'TIMEOUT · STAY', 'ERROR · STAY', 'BUDGET USED', 'EXPIRED · STAY'];
let current = null, frames = [], replay = false, replayFrame = null, previous = null, arrived = 0;
const moves = {'-1': '↑ Up', '0': '— Stay', '1': '↓ Down'};
function show(data) {
  $('left-score').textContent = data.state[6]; $('right-score').textContent = data.state[7];
  $('clock').textContent = `TICK ${String(data.state[8]).padStart(5, '0')}`;
  $('mode').textContent = data.mode === 'jev' ? '● LIVE JEV DECISIONS' : '◌ OFFLINE DEMO · NO API CALLS';
  $('toggle').textContent = data.running ? 'Pause match' : 'Start match ↗';
  $('overlay').style.display = data.running || replay ? 'none' : 'flex';
  data.players.forEach((p, i) => {
    $(`action-${i}`).textContent = moves[p.action];
    $(`thinking-${i}`).textContent = p.thinking ? 'DECIDING…' : (names[p.status] || 'UNKNOWN');
    $(`latency-${i}`).textContent = p.status === 1 || !p.history.length ? '—' : `${p.latency_ms} ms`;
    $(`requests-${i}`).textContent = `${p.requests} / ${data.max_decisions}`;
    const history = $(`history-${i}`); history.replaceChildren();
    p.history.forEach(entry => { const cell = document.createElement('span'); cell.textContent = moves[entry.action].split(' ')[0]; cell.title = `Tick ${entry.tick}: ${names[entry.status]}`; history.append(cell); });
  });
  $('details').textContent = `Decision cadence ≥ ${data.interval_ms} ms · Actions held between decisions · 30 simulation ticks/sec · ${data.mode === 'jev' ? `Up to ${2 * data.max_decisions} API requests per server session` : 'Deterministic tracking bots; no model decisions'}`;
}
async function control(running) {
  const response = await fetch('/api/control', {method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({running})});
  if (!response.ok) throw new Error('Control failed');
  current = await response.json(); previous = current; arrived = performance.now();
  if (!replay) show(current);
}
$('toggle').onclick = async () => { try { replay = false; $('replay-label').textContent = 'Live'; await control(!current.running); } catch (_) { $('connection').textContent = 'Control failed'; } };
$('save').onclick = () => {
  const blob = new Blob([JSON.stringify({schema: 1, mode: current?.mode, frames})], {type: 'application/json'});
  const link = document.createElement('a'); link.href = URL.createObjectURL(blob); link.download = 'keel-pong-replay.json'; link.click(); setTimeout(() => URL.revokeObjectURL(link.href), 1000);
};
$('scrub').oninput = () => { if (!frames.length) return; replay = true; const frame = frames[Number($('scrub').value)]; replayFrame = frame; show(frame); $('replay-label').textContent = `Tick ${frame.state[8]}`; };
$('live').onclick = () => { replay = false; $('replay-label').textContent = 'Live'; if (current) show(current); };
async function poll() {
  try {
    const response = await fetch('/api/state'); if (!response.ok) throw new Error('State failed');
    const data = await response.json(); previous = current || data; current = data; arrived = performance.now();
    if (!frames.length || frames[frames.length - 1].state[8] !== data.state[8]) {
      frames.push(data); if (frames.length > 1800) frames.shift(); $('scrub').max = frames.length - 1;
      if (!replay) $('scrub').value = frames.length - 1;
    }
    $('toggle').disabled = false; $('connection').textContent = '● Connected';
    if (!replay) show(data);
  } catch (_) { $('connection').textContent = 'Disconnected · reconnecting'; $('toggle').disabled = true; }
  setTimeout(poll, 50);
}
function render(now) {
  ctx.clearRect(0, 0, 1000, 600);
  ctx.strokeStyle = '#1c303b'; ctx.lineWidth = 1;
  for (let x = 50; x < 1000; x += 50) {ctx.beginPath();ctx.moveTo(x,0);ctx.lineTo(x,600);ctx.stroke();}
  for (let y = 50; y < 600; y += 50) {ctx.beginPath();ctx.moveTo(0,y);ctx.lineTo(1000,y);ctx.stroke();}
  ctx.setLineDash([6,14]);ctx.strokeStyle='#35505b';ctx.beginPath();ctx.moveTo(500,0);ctx.lineTo(500,600);ctx.stroke();ctx.setLineDash([]);
  const data = replay ? replayFrame : current;
  if (data) {
    const s = [...data.state];
    if (!replay && previous && Math.abs(previous.state[0] - s[0]) < 100) {
      const t = Math.min(1, (now-arrived)/50);
      [0,1,4,5].forEach(i => s[i] = previous.state[i] + (s[i]-previous.state[i])*t);
    }
    [[18,s[4],'#83ead7'],[970,s[5],'#ffa08b']].forEach(([x,y,c]) => {ctx.fillStyle=c;ctx.shadowColor=c;ctx.shadowBlur=15;ctx.fillRect(x,y-55,12,110);});
    ctx.fillStyle='#f4f6df';ctx.shadowColor='#f4f6df';ctx.shadowBlur=16;ctx.beginPath();ctx.arc(s[0],s[1],8,0,Math.PI*2);ctx.fill();ctx.shadowBlur=0;
  }
  requestAnimationFrame(render);
}
poll(); requestAnimationFrame(render);
