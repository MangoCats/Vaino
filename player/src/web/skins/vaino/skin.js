// The reference skin. Whatever a new skin needs from `core.js`, this one uses
// first -- so if something is missing from the contract, it shows up here.
(() => {
  const $ = id => document.getElementById(id);
  const { clock, overlap } = Vaino.fmt;

  // Volume, programme and the queue are the same behaviour in every skin, so
  // they come from core; this skin says only where they live and how a row
  // should read `[REQ-VIS-160]`.
  const showVolume = Vaino.bindVolume($('volume'), $('volnum'));
  const showProgram = Vaino.bindProgram($('prog'));
  const showQueue = Vaino.bindQueue(
    $('queue'), q => (q.artist ? `${q.title} — ${q.artist}` : q.title));

  // Seconds here, milliseconds on the wire: seconds are what the listener is
  // choosing, milliseconds are what the mixer counts in.
  const skipFade = $('skipfade'), skipLead = $('skiplead');
  skipFade.onchange = () => Vaino.skipFade(skipFade.value * 1000);
  skipLead.onchange = () => Vaino.skipLead(skipLead.value * 1000);

  // Each term is shown separately, never just the product: a single number
  // cannot be argued with, and arguing with it is the point [SPEC-DIR-190].
  // Terms sitting at 1.0 did nothing, and are dimmed rather than hidden --
  // "this did not apply" is itself part of the answer.
  const TERMS = [
    ['Artist recovery',  w => w.artist_weight],
    ['Track restraint',  w => w.track_restraint],
    ['Track recovery',   w => w.track_ramp],
    ['Related damping',  w => w.related_damping],
    ['Length bonus',     w => w.length_bonus],
    ['Occasion',         w => w.occasion],
  ];


  // The <option> list is rebuilt only when it actually changes; replacing it on
  // every push would close the dropdown in the user's hand.
  let progSig = '';

  // Limits come from the engine, which is what actually enforces them.
  // Fields being edited are left alone, as the volume slider is while held.
  function renderSkip(k) {
    if (!k) return;
    if (document.activeElement !== skipFade) {
      skipFade.min = 0;
      skipFade.max = k.fade_max_ms / 1000;
      skipFade.value = (k.fade_ms / 1000).toFixed(1);
    }
    if (document.activeElement !== skipLead) {
      skipLead.min = k.lead_min_ms / 1000;
      skipLead.max = k.lead_max_ms / 1000;
      skipLead.value = (k.lead_ms / 1000).toFixed(1);
    }
    $('skipoverlap').textContent = overlap(k);
  }

  function renderWhy(w) {
    const terms = $('terms'), losers = $('losers');
    if (!w) {
      $('why').textContent =
        'This passage was not chosen by the Program Director — it was resumed, ' +
        'or queued before the log began.';
      terms.hidden = losers.hidden = true;
      $('stages').textContent = '';
      return;
    }
    const prog = w.program ? `${w.program}: ` : '';
    $('why').textContent =
      `${prog}weight ${w.decayed_weight.toFixed(3)} of ${w.pool_weight.toFixed(1)} across ` +
      `${w.pool_size.toLocaleString()} candidates — a ` +
      `${w.share_pct.toFixed(2)}% chance of being picked.`;

    const body = terms.querySelector('tbody');
    body.textContent = '';
    for (const [label, get] of TERMS) {
      const v = get(w);
      const tr = body.insertRow();
      if (Math.abs(v - 1) < 1e-9) tr.className = 'inert';
      tr.insertCell().textContent = label;
      tr.insertCell().textContent = '×' + v.toFixed(4);
    }
    const tot = body.insertRow();
    tot.className = 'total';
    tot.insertCell().textContent = 'Frequency weight';
    tot.insertCell().textContent = w.weight.toFixed(4);

    // Stage B/C terms: how it FIT, kept visually apart from how OFTEN it may
    // play. The two stories stay separate all the way to the screen.
    const add = (label, text) => {
      const tr = body.insertRow();
      tr.insertCell().textContent = label;
      tr.insertCell().textContent = text;
    };
    if (w.shaping && !w.shaping.bypassed) {
      add('Pool shaping', `${w.shaping.eligible_in.toLocaleString()} eligible → ` +
                         `${w.shaping.gathered} gathered, ${w.shaping.seeds_used} seeds`);
      if (w.shaping.disliked_out) add('Disliked out', String(w.shaping.disliked_out));
    }
    if (w.seed_distances && w.seed_distances.length) {
      add('Distance to seeds', w.seed_distances.map(d => d.toFixed(2)).join(', '));
    }
    if (w.flow_distance != null) add('Follows previous by', w.flow_distance.toFixed(3));
    add('Flow rank', `#${w.rank + 1}`);
    const decayed = body.insertRow();
    decayed.className = 'total';
    decayed.insertCell().textContent = 'Roulette weight';
    decayed.insertCell().textContent = w.decayed_weight.toFixed(4);
    terms.hidden = false;

    const ol = losers.querySelector('ol');
    ol.textContent = '';
    for (const r of w.runners_up ?? []) {
      const li = document.createElement('li');
      li.textContent = r.title + ' ';
      const span = document.createElement('span');
      span.className = 'w';
      span.textContent = `(${r.weight.toFixed(3)})`;
      li.appendChild(span);
      ol.appendChild(li);
    }
    losers.hidden = ol.children.length === 0;
    $('stages').textContent = w.stages ?? '';
  }

  // Each message is a complete snapshot, so rendering is a pure function of
  // the last one received. No accumulated client state means no drift.
  // Names carry where they came from, because "the MusicBrainz Recording title"
  // and "whatever this file's ID3 tag says" are different claims [REQ-VIS-120].
  // On hover rather than inline: it matters when you ask, not at every glance.
  const SOURCE = { musicbrainz: 'MusicBrainz', tags: 'the file tags',
                   filename: 'the filename', unknown: 'nowhere' };

  Vaino.subscribe(s => {
    $('title').textContent = s.title ?? '—';
    $('title').title = `title from ${SOURCE[s.title_source] ?? s.title_source}`;
    $('byline').textContent = [s.artist, s.album].filter(Boolean).join(' — ');
    $('byline').title =
      `artist from ${SOURCE[s.artist_source]}, album from ${SOURCE[s.album_source]}`;
    $('plays').textContent = Vaino.fmt.plays(s.plays, s.last_played);
    Vaino.showArt($('art'), s.passage_id);
    $('time').textContent = `${clock(s.position_ms)} / ${clock(s.duration_ms)}`;
    $('fill').style.width =
      s.duration_ms ? `${(s.position_ms / s.duration_ms) * 100}%` : '0';
    $('state').textContent = s.playing ? 'playing' : 'paused';
    $('under').textContent = s.underrun_samples;
    $('queuelen').textContent = s.queue_len ?? 0;
    showQueue(s);
    showProgram(s);
    showVolume(s);
    $('progmode').textContent = s.program
      ? (s.program_manual ? `${s.program}, chosen` : `${s.program}, by the clock`)
      : '';
    renderSkip(s.skip);
    renderWhy(s.why);
    $('link').textContent =
      Vaino.status === 'connected' ? 'Connected' : 'Reconnecting…';
  });
})();
