// MuLibPlay's arrangement, driven by Vaino's socket.
//
// Two of its ideas map straight onto Vaino concepts and are kept as they were:
// the stacked station buttons with the live one in gold are the programme list,
// and "Autoselect by clock time" is the manual-programme override [SPEC-DIR-185].
//
// The original "Title by Artist from Album", its 200px cover and its play count
// are all back, now that the engine sends them [REQ-VIS-170]. Its
// browse-by-artist pages are still missing, needing endpoints that do not exist.
//
// The connecting words hide with what they introduce: a bare "by" with nothing
// after it reads as a fault, and about a third of this library has no embedded
// cover at all.
(() => {
  const $ = id => document.getElementById(id);
  const { clock } = Vaino.fmt;

  // The original submitted volume on mouseup and touchend for the same reason
  // core sends on release: a level per pixel of travel would be a request per
  // pixel of travel.
  const showVolume = Vaino.bindVolume($('volume'), $('volnum'));
  const showQueue = Vaino.bindQueue(
    $('queue'), q => (q.artist ? `${q.title} by ${q.artist}` : q.title), 'p');

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


  // Hide the connecting word along with the value it introduces, and let the
  // value carry where it came from `[REQ-VIS-120]`.
  const pair = (wordId, valueId, text, source, what) => {
    Vaino.named($(valueId), text, source, what);
    $(wordId).hidden = !text;
  };

  Vaino.subscribe(s => {
    Vaino.named($('title'), s.title ?? '—', s.title_source, 'title');
    pair('byword', 'artist', s.artist, s.artist_source, 'artist');
    pair('fromword', 'album', s.album, s.album_source, 'album');
    $('plays').textContent = Vaino.fmt.plays(s.plays, s.last_played);
    Vaino.showArt($('art'), s.passage_id);
    $('time').textContent = `${clock(s.position_ms)} / ${clock(s.duration_ms)}`;
    lit($('b-play'), s.playing);
    lit($('b-pause'), !s.playing);
    showVolume(s);
    showQueue(s);
    renderStations(s);
    $('why').textContent = s.why
      ? `${s.why.program ? s.why.program + ': ' : ''}weight ` +
        `${s.why.decayed_weight.toFixed(3)} of ${s.why.pool_weight.toFixed(1)} across ` +
        `${s.why.pool_size.toLocaleString()} candidates — ` +
        `${s.why.share_pct.toFixed(2)}% chance of being picked.`
      : '';
  });
})();
