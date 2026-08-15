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
    if (!(item.suggested || []).length) {
      opts.appendChild(el('li', null, 'nothing named — the match carried no recording'));
    }
    theirs.appendChild(opts);
    claims.appendChild(theirs);
    box.appendChild(claims);

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

    opts.addEventListener('change', () => { use.disabled = !picked(); });
    const picked = () =>
      box.querySelector(`input[name="pick-${item.passage_id}"]:checked`);

    const settle = async (decision, mbid, phrase) => {
      for (const b of [use, keep, later]) b.disabled = true;
      const q = mbid ? `?mbid=${encodeURIComponent(mbid)}` : '';
      const r = await fetch(`/review/${item.passage_id}/${decision}${q}`,
                            { method: 'POST' });
      if (r.ok) {
        // Remembered on the item, not just on the element: toggling a filter
        // re-renders every card, and a settled one coming back armed would
        // invite deciding it twice.
        item.decided = phrase;
        box.classList.add('done');
        said.textContent = phrase;
        tally.decided++;
        showTally();
      } else {
        // Re-arm rather than leaving a dead card: a failed write must not look
        // like a recorded decision.
        said.textContent = `could not record that (${r.status})`;
        keep.disabled = later.disabled = false;
        use.disabled = !picked();
      }
    };

    use.onclick = () => {
      const p = picked();
      if (p) settle('reassigned', p.value, 'reassigned');
    };
    keep.onclick = () => settle('kept', null, 'kept as it was');
    later.onclick = () => settle('deferred', null, 'left for later');

    acts.append(play, el('span', 'spacer'), said, use, keep, later);
    box.appendChild(acts);
    if (item.decided) {
      box.classList.add('done');
      said.textContent = item.decided;
      for (const b of [use, keep, later]) b.disabled = true;
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
  function showFilters() {
    const host = $('filters');
    host.textContent = '';
    const counts = {};
    for (const i of items) counts[i.severity] = (counts[i.severity] || 0) + 1;
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
  }

  function showCards() {
    const cards = $('cards');
    cards.textContent = '';
    const shown = items.filter(i => showing.has(i.severity));
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
