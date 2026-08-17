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
  // Which track the explanation panel is describing `[REQ-VIS-100]`.
  //
  // `null` means "whatever is playing", and it follows the track changes --
  // which is the behaviour this skin had before any of this, so doing nothing
  // still works the way it used to. Picking a queued row pins the panel to
  // that passage until something else is picked or it starts playing.
  let picked = null;
  let pickedWhy = null;          // fetched once; an explanation never changes

  const showQueue = Vaino.bindQueue(
    $('queue'), q => (q.artist ? `${q.title} — ${q.artist}` : q.title), 'li',
    'nothing queued',
    {
      // One control set beside the picked row instead of a set on every row.
      controls: false,
      selected: () => picked,
      onPick: (qid, item) => { pick(qid, item.passage_id); },
    });

  // `qid` identifies the QUEUE ENTRY; `passageId` the recording it plays. The
  // selection follows the entry -- two copies of one passage are separately
  // pickable -- while the explanation is fetched for the passage, because an
  // explanation is about the recording and is the same for both copies.
  function pick(qid, passageId) {
    picked = qid;
    pickedWhy = null;
    if (qid != null && passageId != null) {
      // 404 is a real answer -- a resumed passage, or one queued before the
      // log began -- and renderWhy already says so for null.
      fetch(`/why/${passageId}`)
        .then(r => (r.ok ? r.json() : null))
        .then(w => { if (picked === qid) { pickedWhy = w; draw(); } })
        .catch(() => { if (picked === qid) draw(); });
    }
    draw();
  }
  $('nowrow').onclick = () => pick(null, null);

  // The settings screen `[REQ-VIS-160]`. The skip shape and the programme are
  // set once and then left alone, so they no longer sit in the reading path
  // between the transport and the queue.
  //
  // A panel swap rather than a second page: the settings are driven by the
  // same snapshot as everything else, and a separate page would need its own
  // socket to show the programme currently in force. Both panels stay in the
  // DOM, so the bindings below attach once and keep working either way.
  const gear = $('gear');
  gear.onclick = () => {
    const open = gear.getAttribute('aria-expanded') !== 'true';
    gear.setAttribute('aria-expanded', String(open));
    $('panel-main').hidden = open;
    $('panel-settings').hidden = !open;
  };

  // Seconds here, milliseconds on the wire: seconds are what the listener is
  // choosing, milliseconds are what the mixer counts in.
  const skipFade = $('skipfade'), skipLead = $('skiplead');
  skipFade.onchange = () => Vaino.skipFade(skipFade.value * 1000);
  skipLead.onchange = () => Vaino.skipLead(skipLead.value * 1000);
  const resumeSave = $('resumesave');
  resumeSave.onchange = () => Vaino.resumeSave(resumeSave.value * 1000);

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
    if (k.resume_save_ms != null && document.activeElement !== resumeSave) {
      resumeSave.min = k.resume_save_min_ms / 1000;
      resumeSave.max = k.resume_save_max_ms / 1000;
      resumeSave.value = (k.resume_save_ms / 1000).toFixed(1);
    }
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

  // The byline is two independent claims that happen to share a line, so each
  // half carries its own provenance: a MusicBrainz artist beside an album that
  // is only a file tag is the common case here, and averaging the two into one
  // marker would describe neither.
  function showByline(s) {
    const el = $('byline');
    el.textContent = '';
    const part = (text, source, what) => {
      if (!text) return;
      if (el.firstChild) el.appendChild(document.createTextNode(' — '));
      const span = document.createElement('span');
      Vaino.named(span, text, source, what);
      el.appendChild(span);
    };
    part(s.artist, s.artist_source, 'artist');
    part(s.album, s.album_source, 'album');
  }

  // Each message is a complete snapshot, so rendering is a pure function of
  // the last one received. No accumulated client state means no drift.
  // The last snapshot, so picking a row can redraw immediately instead of
  // waiting up to half a second for the next push.
  let latest = null;
  const draw = () => { if (latest) render(latest); };

  Vaino.subscribe(s => { latest = s; render(s); });

  function render(s) {
    Vaino.named($('title'), s.title ?? '—', s.title_source, 'title');
    showByline(s);
    $('plays').textContent = Vaino.fmt.plays(s.plays, s.last_played);
    Vaino.showArt($('art'), s.passage_id);
    $('time').textContent = `${clock(s.position_ms)} / ${clock(s.duration_ms)}`;
    $('fill').style.width =
      s.duration_ms ? `${(s.position_ms / s.duration_ms) * 100}%` : '0';
    $('state').textContent = s.playing ? 'playing' : 'paused';
    // The ground shifts with it, so the state is legible without reading.
    $('devmode').hidden = !s.dev_mode;
    document.body.classList.toggle('dev', Boolean(s.dev_mode));
    $('under').textContent = s.underrun_samples;
    showQueue(s);
    showProgram(s);
    showVolume(s);
    $('progmode').textContent = s.program
      ? (s.program_manual ? `${s.program}, chosen` : `${s.program}, by the clock`)
      : '';
    renderSkip(s.skip);
    renderPick(s);
    $('link').textContent =
      Vaino.status === 'connected' ? 'Connected' : 'Reconnecting…';
  }

  // The explanation panel, and the one control set that goes with it.
  function renderPick(s) {
    // A pinned passage that has since started playing, or left the queue
    // entirely, falls back to following the current track rather than
    // explaining something no longer there.
    const queued = (s.queue || []).some(q => q.qid === picked);
    if (picked != null && !queued) picked = null;

    $('nowrow').classList.toggle('picked', picked == null);
    const on = picked != null ? (s.queue || []).find(q => q.qid === picked) : null;
    $('whotitle').textContent = on
      ? (on.artist ? `${on.title} — ${on.artist}` : on.title)
      : 'this track';
    renderWhy(picked == null ? s.why : pickedWhy);

    // Controls belong to a queued row; the playing one cannot be moved or
    // dropped, which is why picking it hides them rather than disabling them.
    const box = $('qpick');
    box.hidden = !on;
    if (on) {
      box.textContent = '';
      box.appendChild(Vaino.queueControls(on.qid, on.editable));
    }
  }

  // ------------------------------------------------------------- speakers
  // Choosing a speaker `[PI3-UI-010]`. The states shown are the ones the
  // system reports, not the ones we hoped for: 'playing' and 'connected' are
  // different rows because a speaker can be linked while the audio goes
  // somewhere else entirely, and showing a tick for that is the lie that hid
  // this fault for two days `[PI3-WHY-010]`.
  const STATE_TEXT = {
    playing:   ['Playing here', null],
    connected: ['Connected, but silent', 'Use this one'],
    paired:    ['Known', 'Connect'],
    found:     ['In range', 'Pair'],
    stale:     ['Needs re-pairing', 'Repair'],
  };
  const VERB = { connected: 'use', paired: 'use', found: 'pair', stale: 'repair' };

  let previous = null;   // the speaker to fall back to, for confirm-or-revert
  let countdown = null;

  function speakerRow(d, output) {
    const li = document.createElement('li');
    // A device is only 'playing' if the audio is demonstrably reaching it.
    const playing = d.state === 'connected' && output &&
                    output.sink && output.sink === d.name;
    const state = playing ? 'playing' : d.state;
    const [label, action] = STATE_TEXT[state] || [state, null];
    li.className = 'bt ' + state;

    const name = document.createElement('span');
    name.className = 'btname';
    name.textContent = d.name || d.address;
    const said = document.createElement('span');
    said.className = 'btstate';
    said.textContent = label;
    li.append(name, said);

    if (action) {
      const b = document.createElement('button');
      b.type = 'button';
      b.textContent = action;
      b.onclick = () => choose(d, VERB[state] || 'use');
      li.appendChild(b);
    }
    if (state !== 'found') {
      const f = document.createElement('button');
      f.type = 'button';
      f.className = 'btforget';
      f.textContent = 'Forget';
      f.onclick = () => act('forget', d.address);
      li.appendChild(f);
    }
    return li;
  }

  async function act(verb, address) {
    hint(verb === 'pair' ? 'Pairing — this takes about half a minute…'
                         : 'Working…');
    try {
      const r = await fetch(`/audio/speakers/${verb}/${address}`, { method: 'POST' });
      const body = await r.json().catch(() => null);
      if (!r.ok) { hint(body || 'That did not work.'); return null; }
      return body;
    } catch (e) {
      hint('Could not reach the player.');
      return null;
    }
  }

  // Confirm or revert `[PI3-UI-030]`. Switching speakers can destroy the very
  // means of hearing whether the switch worked, so the change is provisional
  // until someone says they can hear it, and an unanswered question puts the
  // old one back rather than leaving a silent appliance.
  async function choose(device, verb) {
    const before = await sinkNow();
    const body = await act(verb, device.address);
    if (!body) { refresh(); return; }
    if (body.audible === false) {
      hint('Connected, but the sound is not reaching it yet.');
      refresh();
      return;
    }
    previous = before;
    askConfirm(device);
    refresh();
  }

  function askConfirm(device) {
    $('bt-confirm').hidden = false;
    let left = 30;
    const tick = () => {
      $('bt-countdown').textContent =
        `Going back to what worked in ${left}s if you do not answer.`;
      if (left-- <= 0) revert();
    };
    clearInterval(countdown);
    tick();
    countdown = setInterval(tick, 1000);
    $('bt-yes').onclick = () => { settle(); hint(`Playing on ${device.name}.`); };
    $('bt-no').onclick = () => revert();
  }

  function settle() {
    clearInterval(countdown);
    $('bt-confirm').hidden = true;
    previous = null;
  }

  async function revert() {
    const back = previous;
    settle();
    hint('Putting the old speaker back…');
    // Nothing to go back TO is a real case: the previous sink may have been
    // the dummy, which is to say silence. Reopening is still right -- it is
    // how the player is told to look again.
    if (back && back.address) await act('use', back.address);
    else await fetch('/command/reopen-output', { method: 'POST' });
    refresh();
  }

  async function sinkNow() {
    try {
      const r = await fetch('/audio/sink');
      const s = await r.json();
      const list = await (await fetch('/audio/speakers')).json();
      const match = (list.devices || []).find(d => d.name === s.sink);
      return match || null;
    } catch (e) { return null; }
  }

  function hint(text) { $('bt-hint').textContent = text; }

  async function refresh(scan) {
    const list = $('bt-list');
    if (scan) hint('Looking for speakers — about twenty seconds…');
    try {
      const r = await fetch(scan ? '/audio/speakers/scan' : '/audio/speakers',
                            { method: scan ? 'POST' : 'GET' });
      const body = await r.json();
      list.textContent = '';
      const devices = body.devices || [];
      if (!devices.length) {
        hint('No speakers known yet. Put yours in pairing mode, then look.');
        return;
      }
      for (const d of devices) list.appendChild(speakerRow(d, body.output));
      if (!scan) {
        const out = body.output;
        hint(out && out.dummy
             ? 'Nothing can hear the music right now.'
             : out && out.sink ? `Playing on ${out.sink}.` : 'Where the music is playing.');
      } else {
        hint('Pick yours from the list.');
      }
    } catch (e) {
      hint('Could not reach the player.');
    }
  }

  $('bt-scan').onclick = () => refresh(true);
  // Populated when the panel is opened rather than at load: it costs a
  // subprocess on the appliance, and most sessions never open the settings.
  gear.addEventListener('click', () => {
    if (!$('panel-settings').hidden) refresh();
  });

})();
