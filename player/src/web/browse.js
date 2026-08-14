// MuLibPlay's "Browse by: Artist / Album / Track", rebuilt on Vaino's data
// [REQ-VIS-180].
//
// One page for all three skins rather than three implementations. It borrows
// the chosen skin's stylesheet for its ground, type and buttons, and supplies
// only the layout a listing needs -- so a new skin gets browsing for free, and
// can still restyle every part of it.
//
// Artist and Album are ways IN to tracks, not destinations: clicking an artist
// narrows to their albums, an album to its tracks, and a track queues it. The
// crumb trail is what makes that reversible.
(() => {
  const $ = id => document.getElementById(id);

  let kind = 'artists';
  let filter = {};          // { artist, album } -- the narrowing, not the search
  let timer = null;

  function crumbs() {
    const c = $('crumbs');
    c.textContent = '';
    if (!filter.artist && !filter.album) {
      c.textContent = { artists: 'All artists', albums: 'All albums', tracks: 'All tracks' }[kind];
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

  // One row builder for all three listings: cells, an optional click, and the
  // numbers right-aligned. The shapes differ only in what the cells hold.
  function row(cells, onClick) {
    const tr = document.createElement('tr');
    for (const [text, cls] of cells) {
      const td = document.createElement('td');
      td.textContent = text;
      if (cls) td.className = cls;
      tr.appendChild(td);
    }
    if (onClick) {
      tr.classList.add('go');
      tr.onclick = onClick;
    }
    return tr;
  }

  const plural = (n, one) => `${n.toLocaleString()} ${one}${n === 1 ? '' : 's'}`;

  async function show(next) {
    if (next) kind = next;
    for (const b of document.querySelectorAll('[data-kind]')) {
      b.setAttribute('aria-selected', String(b.dataset.kind === kind));
    }
    crumbs();

    const q = $('q').value.trim();
    const rows = await Vaino.browse(kind, { ...filter, q });
    const body = $('rows');
    body.textContent = '';

    if (!rows.length) {
      body.appendChild(row([['nothing matches', '']]));
      $('count').textContent = '';
      return;
    }

    for (const r of rows) {
      if (kind === 'tracks') {
        // Queue rather than navigate: a track is the end of the trail, and
        // the only useful thing left to do with it is hear it.
        body.appendChild(row(
          [[r.title, ''], [r.artist ?? '', 'sub'], [r.album ?? '', 'sub'],
           [r.plays ? `${r.plays}×` : '', 'num'], ['queue', 'num']],
          () => Vaino.queueNext(r.passage_id).then(() => {
            $('count').textContent = `queued ${r.title}`;
          })));
      } else if (kind === 'albums') {
        body.appendChild(row(
          [[r.name, ''], [r.artist ?? '', 'sub'],
           [plural(r.passages, 'track'), 'num'], [`${r.plays}×`, 'num']],
          () => { filter = { artist: r.artist ?? undefined, album: r.name }; show('tracks'); }));
      } else {
        body.appendChild(row(
          [[r.name, ''], [plural(r.passages, 'track'), 'num'], [`${r.plays}×`, 'num']],
          () => { filter = { artist: r.name }; show('albums'); }));
      }
    }
    $('count').textContent = `${rows.length.toLocaleString()} ${kind}`;
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
