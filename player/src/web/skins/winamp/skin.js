// The awkward case, on purpose: a fixed-width appliance rather than a document,
// with its own type, geometry and a scrolling title. It needed nothing from
// core.js that the other two did not -- which is what makes the contract worth
// having rather than merely tidy.
(() => {
  const $ = id => document.getElementById(id);
  const { clock, round1, db: dbLabel, overlap } = Vaino.fmt;

  let holding = false, pendingDb = 0;
  const vol = $('volume');
  vol.oninput = () => {
    holding = true;
    pendingDb = round1(Vaino.fader.db(Number(vol.value)));
    $('volnum').textContent = dbLabel(pendingDb);
  };
  vol.onchange = () => { holding = false; Vaino.volume(pendingDb); };

  $('prog').onchange = e => Vaino.program(e.target.value);
  const skipFade = $('skipfade'), skipLead = $('skiplead');
  skipFade.onchange = () => Vaino.skipFade(skipFade.value * 1000);
  skipLead.onchange = () => Vaino.skipLead(skipLead.value * 1000);

  // Scroll only when it will not fit. A title that fits and scrolls anyway is
  // the habit that made these players tiring to watch.
  let shown = null;
  function renderTitle(text) {
    if (text === shown) return;
    shown = text;
    const m = $('marquee');
    m.classList.remove('roll');
    m.textContent = '';
    const span = document.createElement('span');
    span.id = 'title';
    span.textContent = text;
    m.appendChild(span);
    if (span.scrollWidth > m.clientWidth) {
      // Doubling the text is what makes the wrap seamless at -50%.
      span.textContent = text + '   ';
      m.appendChild(span.cloneNode(true));
      m.classList.add('roll');
    }
  }

  let progSig = '';
  function renderProgram(s) {
    const sel = $('prog');
    const sig = (s.programs || []).map(p => p.id + p.name).join('|');
    if (sig !== progSig) {
      progSig = sig;
      sel.textContent = '';
      const auto = document.createElement('option');
      auto.value = 'auto';
      auto.textContent = 'auto (by clock)';
      sel.appendChild(auto);
      for (const p of s.programs || []) {
        const o = document.createElement('option');
        o.value = p.id;
        o.textContent = `${p.name} ${p.start}`;
        sel.appendChild(o);
      }
    }
    if (!s.program_manual) sel.value = 'auto';
    else {
      const m = (s.programs || []).find(p => p.name === s.program);
      if (m) sel.value = String(m.id);
    }
  }

  function renderQueue(items) {
    const ol = $('queue');
    ol.textContent = '';
    if (!items || !items.length) {
      const li = document.createElement('li');
      li.className = 'empty';
      li.textContent = 'nothing queued';
      ol.appendChild(li);
      return;
    }
    for (const q of items) {
      const li = document.createElement('li');
      li.appendChild(Vaino.queueControls(q.passage_id));
      const t = document.createElement('span');
      t.className = 'qtitle';
      t.textContent = q.artist ? `${q.artist} - ${q.title} ` : q.title + ' ';
      const d = document.createElement('span');
      d.className = 'dur';
      d.textContent = clock(q.duration_ms);
      t.appendChild(d);
      li.appendChild(t);
      ol.appendChild(li);
    }
  }

  Vaino.subscribe(s => {
    // "Artist - Title" is this idiom's own habit, and it happens to put the
    // more identifying half first when the line has to scroll.
    renderTitle(s.artist ? `${s.artist} - ${s.title}` : (s.title ?? '—'));
    Vaino.showArt($('art'), s.passage_id);
    $('time').textContent = clock(s.position_ms);
    $('stat').textContent =
      `${clock(s.duration_ms)} · ${s.queue_len ?? 0} queued` +
      (s.plays ? ` · ${s.plays} plays` : '') +
      (s.underrun_samples ? ` · ${s.underrun_samples} under` : '');
    $('state').textContent =
      Vaino.status === 'connected' ? (s.playing ? '▶ playing' : '❚❚ paused')
                                   : '… reconnecting';
    $('fill').style.width =
      s.duration_ms ? `${(s.position_ms / s.duration_ms) * 100}%` : '0';
    if (!holding) {
      const db = round1(s.volume_db ?? 0);
      vol.value = Vaino.fader.travel(db);
      $('volnum').textContent = dbLabel(db);
    }
    renderProgram(s);
    renderQueue(s.queue);
    if (s.skip) {
      if (document.activeElement !== skipFade) {
        skipFade.min = 0;
        skipFade.max = s.skip.fade_max_ms / 1000;
        skipFade.value = (s.skip.fade_ms / 1000).toFixed(1);
      }
      if (document.activeElement !== skipLead) {
        skipLead.min = s.skip.lead_min_ms / 1000;
        skipLead.max = s.skip.lead_max_ms / 1000;
        skipLead.value = (s.skip.lead_ms / 1000).toFixed(1);
      }
      $('skipoverlap').textContent = overlap(s.skip);
    }
    $('why').textContent = s.why
      ? `${s.why.program ? s.why.program + ': ' : ''}` +
        `${s.why.share_pct.toFixed(2)}% of ${s.why.pool_size.toLocaleString()} ` +
        `candidates, rank #${s.why.rank + 1}`
      : '';
  });
})();
