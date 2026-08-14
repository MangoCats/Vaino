// MuLibPlay's arrangement, driven by Vaino's socket.
//
// Two of its ideas map straight onto Vaino concepts and are kept as they were:
// the stacked station buttons with the live one in gold are the programme list,
// and "Autoselect by clock time" is the manual-programme override [SPEC-DIR-185].
//
// What the original had and this cannot: album art, artist and album names, play
// counts, and the browse-by-artist pages. None of that is in the snapshot -- the
// engine simply does not send it -- and a skin inventing it would be a lie. The
// omissions are the skin's, not the layout's.
(() => {
  const $ = id => document.getElementById(id);
  const { clock, round1, db: dbLabel } = Vaino.fmt;

  let holding = false, pendingDb = 0;
  const vol = $('volume');
  vol.oninput = () => {
    holding = true;
    pendingDb = round1(Vaino.fader.db(Number(vol.value)));
    $('volnum').textContent = dbLabel(pendingDb);
  };
  // The original submitted on mouseup and touchend for exactly this reason:
  // a level per pixel of travel would be a request per pixel of travel.
  vol.onchange = () => { holding = false; Vaino.volume(pendingDb); };

  // Checked means the clock is choosing, which is Vaino's "no manual override".
  $('autoclock').onchange = e => { if (e.target.checked) Vaino.program('auto'); };

  // Gold is "this is what is on" throughout the original -- transport and
  // stations alike -- so it is applied from one place here.
  const lit = (el, on) => { if (el) el.className = on ? 'buttonOn' : 'button'; };

  let stationSig = '';
  function renderStations(s) {
    const host = $('stations');
    const sig = (s.programs || []).map(p => p.id + p.name).join('|');
    if (sig !== stationSig) {
      stationSig = sig;
      host.textContent = '';
      for (const p of s.programs || []) {
        const b = document.createElement('button');
        b.className = 'button';
        b.textContent = p.name;
        b.title = `from ${p.start}`;
        b.onclick = () => Vaino.program(p.id);
        host.appendChild(b);
      }
    }
    for (const b of host.children) lit(b, b.textContent === s.program);
    $('autoclock').checked = !s.program_manual;
  }

  function renderQueue(items) {
    const host = $('queue');
    host.textContent = '';
    if (!items || !items.length) {
      host.textContent = 'nothing queued';
      return;
    }
    for (const q of items) {
      const p = document.createElement('p');
      p.textContent = q.title + ' ';
      const d = document.createElement('span');
      d.className = 'dur';
      d.textContent = clock(q.duration_ms);
      p.appendChild(d);
      host.appendChild(p);
    }
  }

  Vaino.subscribe(s => {
    $('title').textContent = s.title ?? '—';
    $('time').textContent = `${clock(s.position_ms)} / ${clock(s.duration_ms)}`;
    lit($('b-play'), s.playing);
    lit($('b-pause'), !s.playing);
    if (!holding) {
      const db = round1(s.volume_db ?? 0);
      vol.value = Vaino.fader.travel(db);
      $('volnum').textContent = dbLabel(db);
    }
    renderQueue(s.queue);
    renderStations(s);
    $('why').textContent = s.why
      ? `${s.why.program ? s.why.program + ': ' : ''}weight ` +
        `${s.why.decayed_weight.toFixed(3)} of ${s.why.pool_weight.toFixed(1)} across ` +
        `${s.why.pool_size.toLocaleString()} candidates — ` +
        `${s.why.share_pct.toFixed(2)}% chance of being picked.`
      : '';
  });
})();
