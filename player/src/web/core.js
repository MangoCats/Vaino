// The player, without a single opinion about how it looks [REQ-VIS-160].
//
// Everything a skin needs and nothing it should have to reinvent: the socket,
// the snapshot, the commands, and the two pieces of arithmetic that are
// specified rather than decorative. A skin supplies markup, a stylesheet, and a
// render function; it never opens a socket, never builds a URL, and never
// carries a copy of the fader curve.
//
// The split is not new -- it is the one the server already draws. `/ws` pushes
// complete snapshots and the POST endpoints take commands; that contract is the
// real interface, and the DOM was only ever one rendering of it.
const Vaino = (() => {
  const listeners = [];
  let last = null;

  // ---- commands ----------------------------------------------------------
  // Every one is fire-and-forget. The truth comes back in the next snapshot,
  // so nothing here updates the display optimistically and then has to explain
  // itself when the engine disagrees.
  const post = path => fetch(path, { method: 'POST' });

  // ---- the fader ---------------------------------------------------------
  // Quadratic in travel, flat where it meets full scale [REQ-AUD-156]. This
  // lives here, not in a skin: it is a specified control law, and three skins
  // each carrying their own copy is three chances to disagree with the engine.
  //
  // The floor comes from the engine, so the number exists in exactly one place.
  let faderFloor = -72;
  const fader = {
    // Travel (0 at the left, 1 at the right) to dB.
    db: x => faderFloor * (1 - x) ** 2,
    // dB back to travel, for putting the knob where the engine says it is.
    travel: db => 1 - Math.sqrt(db / faderFloor),
    get floor() { return faderFloor; },
  };

  // ---- formatting --------------------------------------------------------
  const fmt = {
    clock(ms) {
      const t = Math.max(0, Math.round(ms / 1000));
      return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`;
    },
    // Displayed to a tenth of a dB, and SENT as the displayed figure, so the
    // caption cannot differ from the level in force.
    round1: v => Math.round(v * 10) / 10,
    db: db => `${db <= -0.05 ? '−' + Math.abs(db).toFixed(1) : '0.0'} dB`,
    // "12 plays", and when. Play counts are per RECORDING, so the phrasing
    // must not imply this file: the same recording reached through two files
    // is the same thing heard twice.
    plays(n, at) {
      if (!n) return 'never played';
      const times = n === 1 ? 'once' : `${n} times`;
      return at ? `played ${times}, last ${new Date(at * 1000).toLocaleDateString()}`
                : `played ${times}`;
    },
    // "1.5 s of overlap" / "0.5 s of silence between" / "back to back".
    // A lead longer than the fade is legal and leaves a gap; say so plainly
    // rather than letting silence come as a surprise.
    overlap(k) {
      const s = (k.fade_ms - k.lead_ms) / 1000;
      return s > 0 ? `${s.toFixed(1)} s of overlap`
           : s < 0 ? `${(-s).toFixed(1)} s of silence between`
           : 'back to back';
    },
  };

  // ---- cover art ---------------------------------------------------------
  // A URL and the load/error dance around it. The URL because core owns every
  // route; the dance because all three skins would otherwise carry the same
  // eight lines, and art is missing often enough -- roughly a third of this
  // library -- that getting the failure case wrong would be conspicuous.
  //
  // Nothing is asked of the server until a skin asks: a 404 is the normal
  // answer for a file with no embedded picture, not an error to report.
  function showArt(img, passageId) {
    if (!img) return;
    if (passageId == null) {
      img.hidden = true;
      img.removeAttribute('data-for');
      return;
    }
    if (img.dataset.for === String(passageId)) return; // already showing it
    img.dataset.for = String(passageId);
    img.hidden = true;                 // stay hidden until it is known to exist
    img.onload = () => { img.hidden = false; };
    img.onerror = () => { img.hidden = true; };
    img.src = `/art/${passageId}`;
  }

  // ---- skins -------------------------------------------------------------
  // The choice is per browser, not per player: two people on two phones may
  // want different skins of the same radio, and neither should be able to
  // restyle the other. That is why it lives in localStorage and not the engine.
  const KEY = 'vaino.skin';
  let catalogue = [];

  function chosen() {
    const q = new URLSearchParams(location.search).get('skin');
    if (q) localStorage.setItem(KEY, q);
    return q || localStorage.getItem(KEY) || 'vaino';
  }

  function setSkin(name) {
    localStorage.setItem(KEY, name);
    location.href = location.pathname; // drop ?skin= so it is not sticky twice
  }

  // A skin is markup, a stylesheet and a script, in that order: the script may
  // assume its own DOM is present, which is the whole reason it loads last.
  async function loadSkin(name) {
    const base = `/skin/${name}`;
    const [html] = await Promise.all([
      fetch(`${base}/skin.html`).then(r => r.text()),
      new Promise((ok, no) => {
        const l = document.createElement('link');
        l.rel = 'stylesheet';
        l.href = `${base}/skin.css`;
        l.onload = ok;
        l.onerror = no;
        document.head.appendChild(l);
      }),
    ]);
    document.getElementById('app').innerHTML = html;
    // Buttons are wired centrally so no skin has to know a URL to be playable.
    for (const b of document.querySelectorAll('[data-cmd]')) {
      b.onclick = () => post(`/command/${b.dataset.cmd}`);
    }
    for (const b of document.querySelectorAll('[data-skin]')) {
      b.onclick = () => setSkin(b.dataset.skin);
    }
    // A skin marks up an empty <select data-skins> and gets a working picker;
    // the catalogue comes from the server, so adding a skin never means editing
    // three others to list it.
    for (const sel of document.querySelectorAll('[data-skins]')) {
      sel.textContent = '';
      for (const s of catalogue) {
        const o = document.createElement('option');
        o.value = s.name;
        o.textContent = s.label;
        sel.appendChild(o);
      }
      sel.value = chosen();
      sel.onchange = () => setSkin(sel.value);
    }
    await new Promise((ok, no) => {
      const s = document.createElement('script');
      s.src = `${base}/skin.js`;
      s.onload = ok;
      s.onerror = no;
      document.body.appendChild(s);
    });
    // A skin that loads after the first snapshot must not sit blank waiting for
    // the next one, which may be half a second away.
    if (last) dispatch(last);
  }

  function dispatch(s) {
    last = s;
    if (s.fader_min_db) faderFloor = s.fader_min_db;
    for (const fn of listeners) {
      // One skin's mistake must not silence the rest of the page, and a render
      // that throws every 500 ms would otherwise fill the console and stop.
      try { fn(s); } catch (e) { console.error('skin render failed', e); }
    }
  }

  // ---- the socket --------------------------------------------------------
  // Reconnect rather than go stale: the player outlives any one page load, and
  // a silently dead socket looks exactly like paused playback.
  let status = 'connecting';
  function connect() {
    const ws = new WebSocket(`ws://${location.host}/ws`);
    ws.onmessage = e => dispatch(JSON.parse(e.data));
    ws.onopen = () => { status = 'connected'; };
    ws.onclose = () => {
      status = 'reconnecting';
      if (last) dispatch(last); // let skins show the disconnection
      setTimeout(connect, 1000);
    };
  }

  return {
    subscribe(fn) { listeners.push(fn); if (last) fn(last); },
    get snapshot() { return last; },
    get status() { return status; },
    get skins() { return catalogue; },
    get skin() { return chosen(); },
    setSkin,
    fader,
    fmt,
    showArt,
    artUrl: id => `/art/${id}`,
    command: name => post(`/command/${name}`),
    volume: db => post(`/volume/${db}`),
    program: id => post(`/program/${id}`),
    skipFade: ms => post(`/skip/fade/${Math.round(ms)}`),
    skipLead: ms => post(`/skip/lead/${Math.round(ms)}`),
    // The player page: load the chosen skin, then follow the socket.
    async start() {
      catalogue = await fetch('/skins').then(r => r.json()).catch(() => []);
      await loadSkin(chosen());
      connect();
    },
    // The browse page: it wants the skin's LOOK and the command helpers, but
    // not the player's markup and not a socket -- a library listing does not
    // change twice a second.
    async startBare() {
      catalogue = await fetch('/skins').then(r => r.json()).catch(() => []);
      const l = document.createElement('link');
      l.rel = 'stylesheet';
      l.href = `/skin/${chosen()}/skin.css`;
      document.head.appendChild(l);
    },
    browse: (kind, filter = {}) => {
      const q = new URLSearchParams();
      for (const [k, v] of Object.entries(filter)) if (v) q.set(k, v);
      return fetch(`/browse/${kind}?${q}`).then(r => r.json());
    },
    queueNext: id => post(`/queue/${id}`),
  };
})();
