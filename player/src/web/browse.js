// MuLibPlay's "Browse by: Artist / Album / Track", rebuilt on Vaino's data
// [REQ-VIS-180].
//
// One page for all three skins rather than three implementations: it borrows
// the chosen skin's stylesheet and supplies only the layout a listing needs, so
// a new skin gets browsing for free and can still restyle every part of it.
//
// Built for a phone, which is how these pages were actually used. That means an
// alphabet bar rather than a scrollbar -- 463 artists is a long way to drag with
// a thumb, and one tap to the letter is the whole difference.
//
// Artist and album are ways IN to tracks, not destinations: an artist narrows to
// their albums, an album to its tracks, a track queues itself. The crumb trail
// is what makes that reversible.
(() => {
  const $ = id => document.getElementById(id);

  let kind = 'artists';
  let filter = {};          // { artist, album } -- the narrowing, not the search
  let timer = null;

  // The heading must be derived the same way the server sorts, or the letters
  // stop being monotonic: strip a leading "The" here while the ORDER BY does
  // not, and "The Beatles" emits a stray B in the middle of the Ts. So this
  // takes the first character as it stands. Everything outside A-Z lands under
  // '#', which is where "10,000 Maniacs" and "'Til Tuesday" actually sort.
  const initial = name => {
    const c = (name || '').trim().toUpperCase()[0] || '#';
    return c >= 'A' && c <= 'Z' ? c : '#';
  };

  function crumbs() {
    const c = $('crumbs');
    c.textContent = '';
    if (!filter.artist && !filter.album) {
      c.textContent = { artists: 'All artists', albums: 'All albums',
                        tracks: 'All tracks' }[kind];
      return;
    }
    const all = document.createElement('a');
    all.textContent = 'All artists';
    all.onclick = () => { filter = {}; show('artists'); };
    c.appendChild(all);
    if (filter.artist) {
      c.append(' › ');
      const a = document.createElement('a');
      a.textContent = filter.artist;
      a.onclick = () => { filter = { artist: filter.artist }; show('albums'); };
      c.appendChild(a);
    }
    if (filter.album) c.append(' › ' + filter.album);
  }

  // One row shape for all three listings: a name, an optional second line, some
  // numbers, and a tap. Only the contents differ.
  function row(title, sub, nums, onTap) {
    const li = document.createElement('li');
    const name = document.createElement('div');
    name.className = 'name';
    const b = document.createElement('b');
    b.textContent = title;
    name.appendChild(b);
    if (sub) {
      const s = document.createElement('span');
      s.textContent = sub;
      name.appendChild(s);
    }
    li.appendChild(name);
    if (nums) {
      const n = document.createElement('div');
      n.className = 'num';
      n.textContent = nums;
      li.appendChild(n);
    }
    if (onTap) li.onclick = onTap;
    return li;
  }

  // The alphabet bar. Letters with nothing behind them are shown disabled
  // rather than hidden, so the bar does not reflow under the thumb as the
  // listing changes.
  function alphabet(anchors) {
    const bar = $('az');
    bar.textContent = '';
    const letters = ['#', ...'ABCDEFGHIJKLMNOPQRSTUVWXYZ'];
    for (const L of letters) {
      const b = document.createElement('button');
      b.textContent = L;
      const target = anchors.get(L);
      if (target) {
        b.onclick = () => target.scrollIntoView({ block: 'start', behavior: 'smooth' });
      } else {
        b.disabled = true;
      }
      bar.appendChild(b);
    }
  }

  const plural = (n, one) => `${n.toLocaleString()} ${one}${n === 1 ? '' : 's'}`;

  async function show(next) {
    if (next) kind = next;
    for (const b of document.querySelectorAll('[data-kind]')) {
      b.setAttribute('aria-selected', String(b.dataset.kind === kind));
    }
    crumbs();
    const body = $('rows');
    const note = $('note');

    let rows;
    try {
      rows = await Vaino.browse(kind, { ...filter, q: $('q').value.trim() });
    } catch (e) {
      // Say so. The first version of this page rendered a failed query as an
      // empty list, which is indistinguishable from an empty library and sent
      // the fault-finding in entirely the wrong direction.
      body.textContent = '';
      $('az').textContent = '';
      note.className = 'note bad';
      note.textContent = `Could not read the library: ${e.message}`;
      return;
    }

    body.textContent = '';
    note.className = 'note';
    if (!rows.length) {
      note.textContent = kind === 'albums'
        ? 'No albums. Album names come from the files’ own tags — run tagscan over the library.'
        : 'Nothing matches.';
      $('az').textContent = '';
      return;
    }

    // Letter headings double as the jump targets, so the bar needs no separate
    // index and cannot fall out of step with what is on screen.
    const anchors = new Map();
    let last = null;
    for (const r of rows) {
      const label = kind === 'tracks' ? r.title : r.name;
      const L = initial(label);
      if (L !== last) {
        last = L;
        const head = document.createElement('li');
        head.className = 'letter';
        head.textContent = L;
        body.appendChild(head);
        if (!anchors.has(L)) anchors.set(L, head);
      }

      if (kind === 'tracks') {
        body.appendChild(row(
          r.title,
          [r.artist, r.album].filter(Boolean).join(' — '),
          r.plays ? `${r.plays}×` : '',
          () => Vaino.queueNext(r.passage_id).then(() => {
            note.textContent = `Queued ${r.title} — it plays next.`;
          })));
      } else if (kind === 'albums') {
        body.appendChild(row(
          r.name, r.artist ?? '',
          `${plural(r.passages, 'track')}\n${r.plays}×`,
          () => { filter = { artist: r.artist ?? undefined, album: r.name };
                  show('tracks'); }));
      } else {
        body.appendChild(row(
          r.name, '',
          `${plural(r.passages, 'track')}\n${r.plays}×`,
          () => { filter = { artist: r.name }; show('albums'); }));
      }
    }
    alphabet(anchors);
    note.textContent = `${rows.length.toLocaleString()} ${kind}`
      + (rows.length >= 2000 ? ' (showing the first 2,000)' : '');
  }

  for (const b of document.querySelectorAll('[data-kind]')) {
    // Switching axis clears the narrowing: "all albums" after picking one
    // artist is otherwise impossible to ask for.
    b.onclick = () => { filter = {}; show(b.dataset.kind); };
  }
  // Debounced, because every keystroke is a query over the whole library.
  $('q').oninput = () => {
    clearTimeout(timer);
    timer = setTimeout(() => show(), 200);
  };

  // MuLibPlay's three buttons went straight to their own page, so a skin can
  // still ask for one axis by name.
  const asked = new URLSearchParams(location.search).get('kind');
  const start = ['artists', 'albums', 'tracks'].includes(asked) ? asked : 'artists';
  Vaino.startBare().then(() => show(start));
})();
