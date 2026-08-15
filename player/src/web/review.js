// Reviewing questionable recording ids [REQ-LIB-165].
//
// The queue here is only the CONTRADICTED passages: audio that AcoustID
// matched confidently to something other than the id the passage carries.
// Everything else is deliberately absent. `unmatched` in particular is not a
// finding -- it means AcoustID has no entry for the audio, which says nothing
// about whether the stored id is right -- and there are thousands of those.
// Putting them in front of a person would bury the real cases.
//
// Nothing here rewrites the library. A decision is recorded against the
// passage and `tools/apply_reviews.py` folds accepted ones in as a separate,
// deliberate step: reassigning an id changes what a passage *is*, and play
// history is keyed by recording, so doing it silently from a web click would
// re-attribute every past play of it.
(() => {
  const $ = id => document.getElementById(id);
  const el = (tag, cls, text) => {
    const n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text != null) n.textContent = text;
    return n;
  };

  Vaino.startBare();

  const pct = s => (s == null ? '' : `${(s * 100).toFixed(1)}%`);

  // The grades, worst first. `on` is whether it is shown to begin with:
  // `different-id` is 93% of the findings on this library and is a tidiness
  // problem, and `unverified` is not evidence of anything at all -- leaving
  // either on by default would bury the cases actually worth judging under
  // the ones that are not. Both stay one tap away.
  const GRADE = {
    'no-mbid':      { label: 'no MBID',       on: true,
                      why: 'no MusicBrainz id at all — a migration placeholder' },
    'wrong-song':   { label: 'wrong song',    on: true,
                      why: 'neither the title nor the performer matches' },
    'wrong-artist': { label: 'wrong artist',  on: true,
                      why: 'same title, different performer' },
    'wrong-title':  { label: 'wrong title',   on: true,
                      why: 'same performer, different title' },
    'different-id': { label: 'different id',  on: false,
                      why: 'the same recording under another MBID' },
    'unverified':   { label: 'unverified',    on: false,
                      why: 'AcoustID does not know this audio; not evidence either way' },
  };
  const ORDER = Object.keys(GRADE);
  const showing = new Set(ORDER.filter(k => GRADE[k].on));
  // Decided cards are their own chip rather than a grade: a judgement can be
  // any severity, and what you want when looking for one is "the ones I have
  // answered", not "the wrong-artist ones I have answered".
  let showDecided = false;
  // How a recorded decision reads when the card is met again.
  const SAID = { kept: 'kept as it was', reassigned: 'reassigned',
                 deferred: 'left for later' };

  // One card: the two competing claims, then what can be done about it.
  function card(item) {
    const box = el('div', 'card');
    box.dataset.passage = item.passage_id;

    const head = el('div', 'head');
    // Very different findings arrive in this queue and they do not deserve the
    // same attention. Most of the time the audio matches a DIFFERENT RECORDING
    // OF THE SAME SONG -- a remaster, a 5.1 mix, a compilation's own entry --
    // which is tidiness. Occasionally the names disagree too, and that is a
    // passage playing under the wrong name. Saying which is which, and letting
    // one be looked at without the other, is most of the value of the page.
    const kind = el('span', `kind ${item.severity}`, GRADE[item.severity].label);
    kind.title = GRADE[item.severity].why;
    head.appendChild(kind);
    head.appendChild(el('span', 'score',
      `passage ${item.passage_id} · fingerprint match ${pct(item.score)}`));
    box.appendChild(head);

    const claims = el('div', 'claims');

    const mine = el('div', 'claim');
    mine.appendChild(el('h2', null, 'The library says'));
    mine.appendChild(el('div', 'name', item.title || '(untitled)'));
    mine.appendChild(el('div', 'sub',
      [item.artist, item.album].filter(Boolean).join(' — ') || '—'));
    mine.appendChild(el('div', 'mbid', item.stored_mbid));
    claims.appendChild(mine);

    const theirs = el('div', 'claim');
    theirs.appendChild(el('h2', null, 'The audio says'));
    const opts = el('ul', 'opts');
    // Radio rather than a button per candidate: picking is not deciding, and
    // a row of "use this one" buttons makes an irreversible-feeling choice out
    // of what should be a glance.
    for (const s of item.suggested || []) {
      const li = el('li');
      const input = document.createElement('input');
      input.type = 'radio';
      input.name = `pick-${item.passage_id}`;
      input.value = s.mbid;
      input.id = `pick-${item.passage_id}-${s.mbid}`;
      const label = document.createElement('label');
      label.htmlFor = input.id;
      label.appendChild(el('div', 'name', s.title || '(untitled)'));
      label.appendChild(el('div', 'sub', s.artist || '—'));
      label.appendChild(el('div', 'mbid', s.mbid));
      li.appendChild(input);
      li.appendChild(label);
      li.appendChild(el('span', 'pct', pct(s.score)));
      opts.appendChild(li);
    }
    // Nothing to choose between. On this library that is 23 of the 44 `no-mbid`
    // cards and every `unverified` one -- and `no-mbid` leads the queue, so it
    // is most likely the first thing anyone meets. Say what the card is for
    // instead of leaving a dead control to be poked at.
    if (!(item.suggested || []).length) {
      opts.appendChild(el('li', 'none',
        item.severity === 'no-mbid'
          ? 'This passage carries no MusicBrainz id, and AcoustID does not ' +
            'recognise the audio either — so there is nothing here to reassign ' +
            'it to. It needs identifying by hand, or a better fingerprint.'
          : 'AcoustID has no entry for this audio. That is not evidence for or ' +
            'against the stored id — there is simply nothing to compare.'));
    }
    theirs.appendChild(opts);
    claims.appendChild(theirs);
    box.appendChild(claims);

    // Which album to call it. A recording is on many releases -- the album,
    // the remaster, three compilations -- and without an answer the name is
    // picked by release date, which is a guess. Fetched only when a candidate
    // is chosen, because most cards are never reassigned.
    const albums = el('div', 'albums');
    albums.hidden = true;
    box.appendChild(albums);

    let releaseChoice = null;
    async function offerAlbums(mbid) {
      albums.textContent = '';
      albums.hidden = false;
      releaseChoice = null;
      albums.appendChild(el('h2', null, 'Call the album'));
      let list = [];
      try {
        const r = await fetch(`/review/releases/${encodeURIComponent(mbid)}`);
        if (r.ok) list = await r.json();
      } catch { /* offered, not required: the album can stay as it is */ }
      if (!list.length) {
        albums.appendChild(el('p', 'sub',
          'No releases known for this recording yet — the album will keep ' +
          'coming from the file’s own tag.'));
        return;
      }
      const sel = document.createElement('select');
      const none = document.createElement('option');
      none.value = '';
      none.textContent = 'leave the album as it is';
      sel.appendChild(none);
      for (const rel of list) {
        const o = document.createElement('option');
        o.value = rel.mbid;
        o.textContent = [rel.title, rel.date && rel.date.slice(0, 4),
                         rel.status, rel.track_count && `${rel.track_count} tracks`]
                          .filter(Boolean).join(' · ');
        if (rel.chosen) o.selected = true;
        sel.appendChild(o);
      }
      releaseChoice = () => sel.value;
      albums.appendChild(sel);
    }

    const acts = el('div', 'acts');
    const said = el('span', 'said');

    // Hearing it is the only thing that settles a case the names cannot, and
    // the player is already there to do it. Reuses the queue verb rather than
    // growing an audio endpoint of its own [REQ-VIS-185].
    const play = el('button', null, 'Play now');
    play.title = 'send this passage to the player, to hear which it is';
    play.onclick = () => {
      Vaino.queue(item.passage_id, 'now');
      said.textContent = 'sent to the player';
    };

    const use = el('button', null, 'Use the match');
    use.disabled = true;
    const keep = el('button', null, 'Keep ours');
    const later = el('button', null, 'Decide later');

    const hasCandidates = (item.suggested || []).length > 0;
    // A control that can never work should not be offered at all. Presenting it
    // greyed says "not yet"; the truth is "not ever, on this card".
    if (!hasCandidates) use.hidden = true;

    opts.addEventListener('change', () => {
      const p = picked();
      use.disabled = !p;
      // Why it is disabled, where the disabled thing is. Without this the
      // precondition is invisible: the radios read as decoration, and the
      // button reads as broken.
      use.title = p ? 'record this as the right recording'
                    : 'choose one of the matches above first';
      if (p) offerAlbums(p.value);
    });
    use.title = hasCandidates ? 'choose one of the matches above first'
                              : 'nothing to reassign this to';
    const picked = () =>
      box.querySelector(`input[name="pick-${item.passage_id}"]:checked`);

    const settle = async (decision, mbid, phrase) => {
      for (const b of [use, keep, later]) b.disabled = true;
      const q = new URLSearchParams();
      if (mbid) q.set('mbid', mbid);
      const rel = releaseChoice && releaseChoice();
      if (rel) q.set('release', rel);
      const qs = q.toString() ? `?${q}` : '';
      const r = await fetch(`/review/${item.passage_id}/${decision}${qs}`,
                            { method: 'POST' });
      if (r.ok) {
        // Remembered on the item, not just on the element: toggling a filter
        // re-renders every card, and a settled one coming back armed would
        // invite deciding it twice.
        item.decided = phrase;
        box.classList.add('done');
        said.textContent = phrase;
        reopen.hidden = false;
        tally.decided++;
        showTally();
        showFilters();
      } else {
        // Re-arm rather than leaving a dead card: a failed write must not look
        // like a recorded decision. The server sends the reason as text, and
        // "already applied to the library" is exactly what has to be read.
        const why = await r.text().catch(() => '');
        said.textContent = why || `could not record that (${r.status})`;
        keep.disabled = later.disabled = false;
        use.disabled = !picked();
      }
    };

    // Undo. Withdrawing a judgement that has only been recorded is a delete;
    // one already written into the library is not, and the server refuses it
    // with the reason, which lands in `said` above.
    const reopen = el('button', null, 'Undo');
    reopen.title = 'withdraw this decision and put the card back in the queue';
    reopen.onclick = async () => {
      reopen.disabled = true;
      const r = await fetch(`/review/${item.passage_id}/reopen`, { method: 'POST' });
      if (r.ok) {
        delete item.decision;
        delete item.decided;
        tally.decided = Math.max(0, tally.decided - 1);
        showTally();
        showFilters();
        showCards();
      } else {
        said.textContent = (await r.text().catch(() => '')) ||
                           `could not undo that (${r.status})`;
        reopen.disabled = false;
      }
    };

    use.onclick = () => {
      const p = picked();
      if (p) settle('reassigned', p.value, 'reassigned');
    };
    keep.onclick = () => settle('kept', null, 'kept as it was');
    later.onclick = () => settle('deferred', null, 'left for later');

    acts.append(play, el('span', 'spacer'), said, use, keep, later, reopen);
    box.appendChild(acts);

    // A judgement already on record -- from this session or a previous one --
    // renders settled, with undo as the only live control.
    const already = item.decided || (item.decision && SAID[item.decision]);
    if (already) {
      box.classList.add('done');
      said.textContent = item.applied ? `${already}, applied to the library` : already;
      for (const b of [use, keep, later]) b.disabled = true;
    } else {
      reopen.hidden = true;
    }
    return box;
  }

  let tally = {};
  function showTally() {
    const t = tally;
    if (!t.ran) {
      $('tally').textContent =
        'The fingerprint pass has not been run against this library yet.';
      return;
    }
    const left = t.contradicted - t.decided;
    $('tally').innerHTML =
      `<b>${t.checked.toLocaleString()}</b> passages checked against their audio · ` +
      `<b>${t.confirmed.toLocaleString()}</b> confirmed · ` +
      `<b>${t.contradicted.toLocaleString()}</b> contradicted · ` +
      `<b>${Math.max(0, left).toLocaleString()}</b> still to review`;
  }

  async function load() {
    $('note').className = 'note';
    $('note').textContent = 'loading…';
    let data;
    try {
      const r = await fetch('/review/queue');
      if (!r.ok) throw new Error(`the server answered ${r.status}`);
      data = await r.json();
    } catch (e) {
      // A failed query must never render as "nothing to review" -- that is the
      // mistake that made the browse page look empty when it was broken.
      $('note').className = 'note bad';
      $('note').textContent = `Could not read the review queue: ${e.message}`;
      return;
    }
    tally = data.progress || {};
    showTally();
    items = data.items || [];
    showFilters();
    showCards();
  }

  // One chip per grade, each carrying its own count, so the size of each kind
  // of problem is visible before deciding which to work through.
  const isDecided = i => Boolean(i.decision || i.decided);

  function showFilters() {
    const host = $('filters');
    host.textContent = '';
    const counts = {};
    for (const i of items) if (!isDecided(i)) counts[i.severity] = (counts[i.severity] || 0) + 1;
    for (const key of ORDER) {
      const n = counts[key] || 0;
      const b = el('button', `chip ${key}`, `${GRADE[key].label} ${n}`);
      b.title = GRADE[key].why;
      b.disabled = n === 0;
      b.setAttribute('aria-pressed', String(showing.has(key)));
      b.onclick = () => {
        if (showing.has(key)) showing.delete(key); else showing.add(key);
        b.setAttribute('aria-pressed', String(showing.has(key)));
        showCards();
      };
      host.appendChild(b);
    }
    // Decided, so a judgement can be found again and undone. Off by default:
    // the queue's job is to shorten as it is worked through.
    const n = items.filter(isDecided).length;
    const d = el('button', 'chip decided', `decided ${n}`);
    d.title = 'judgements already recorded — open one to undo it';
    d.disabled = n === 0;
    d.setAttribute('aria-pressed', String(showDecided));
    d.onclick = () => {
      showDecided = !showDecided;
      d.setAttribute('aria-pressed', String(showDecided));
      showCards();
    };
    host.appendChild(d);
  }

  function showCards() {
    const cards = $('cards');
    cards.textContent = '';
    const shown = items.filter(i =>
      isDecided(i) ? showDecided : showing.has(i.severity));
    for (const item of shown) cards.appendChild(card(item));

    $('note').textContent =
      !tally.ran
        ? 'Run tools/fingerprint_ids.py, then --merge, to populate this.'
        : !items.length
          ? 'Nothing left to review.'
          : !shown.length
            ? 'Nothing shown — every grade is switched off.'
            : `${shown.length} of ${items.length} shown. Worst first. ` +
              '"Unverified" means AcoustID has no entry for the audio, which ' +
              'is not evidence either way.';
  }

  let items = [];
  load();
})();
