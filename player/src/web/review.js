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

  // One card: the two competing claims, then what can be done about it.
  function card(item) {
    const box = el('div', 'card');
    box.dataset.passage = item.passage_id;

    const head = el('div', 'head');
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
    const cards = $('cards');
    cards.textContent = '';
    for (const item of data.items || []) cards.appendChild(card(item));

    $('note').textContent =
      !tally.ran
        ? 'Run tools/fingerprint_ids.py, then --merge, to populate this.'
        : (data.items || []).length
          ? 'Only ids the audio positively contradicts are listed. ' +
            'Passages AcoustID does not recognise are not evidence of anything ' +
            'and are left out.'
          : 'Nothing left to review.';
  }

  load();
})();
