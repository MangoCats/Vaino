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
    $('queue'), q => (q.artist ? `${q.title} by ${q.artist}` : q.title), 'p',
    'nothing queued',
    // Title/artist become their own clickable spans, reaching the
    // preference panel `[REQ-VIS-285]`.
    { linkable: true, artistJoin: ' by ' });

  // Checked means the clock is choosing, which is Vaino's "no manual override".
  // Unchecking has to name something to switch TO -- "manual" is a programme
  // id, not a bare flag -- so it freezes on whatever is engaged right now
  // rather than doing nothing and leaving the box to spring back on the next
  // snapshot, which is what a `false` branch here used to be `[SPEC-DIR-185]`.
  let activeProgramId = null;
  $('autoclock').onchange = e => {
    if (e.target.checked) { Vaino.program('auto'); }
    else if (activeProgramId != null) { Vaino.program(activeProgramId); }
  };

  // Gold is "this is what is on" throughout the original -- transport and
  // stations alike -- so it is applied from one place here.
  const lit = (el, on) => { if (el) el.className = on ? 'buttonOn' : 'button'; };

  let stationSig = '';
  function renderStations(s) {
    const host = $('stations');
    // By engagement time, not by name -- the order the list is actually
    // used in, since that is the question "what comes on when" asks.
    const sorted = [...(s.programs || [])].sort((a, b) => a.start.localeCompare(b.start));
    const active = sorted.find(p => p.name === s.program);
    activeProgramId = active ? active.id : null;
    const sig = sorted.map(p => p.id + p.name + p.start).join('|');
    if (sig !== stationSig) {
      stationSig = sig;
      host.textContent = '';
      for (const p of sorted) {
        const row = document.createElement('div');
        row.className = 'stationrow';
        const time = document.createElement('span');
        time.className = 'engagetime';
        time.textContent = p.start;
        const b = document.createElement('button');
        b.className = 'button';
        b.textContent = p.name;
        b.title = `from ${p.start}`;
        b.onclick = () => Vaino.program(p.id);
        row.append(time, b);
        host.appendChild(row);
      }
    }
    for (const b of host.querySelectorAll('.button, .buttonOn')) lit(b, b.textContent === s.program);
    // Dimmed once a manual pick has made the schedule inert -- the same
    // signal the checkbox itself gives, read here for the times beside it.
    for (const t of host.querySelectorAll('.engagetime')) t.classList.toggle('dim', s.program_manual);
    $('autoclock').checked = !s.program_manual;
  }


  // Names are shown bare in this skin `[REQ-VIS-122]`. The provenance marks
  // -- MB, tag, file -- are a Vaino idea the original never had, and this skin
  // is a reproduction of its face. The claim is not abandoned, only moved: the
  // Vaino and WinAmp skins still carry it on every name.
  const plain = (el, text) => { if (el) el.textContent = text ?? ''; };

  // Hide the connecting word along with the value it introduces.
  const pair = (wordId, valueId, text) => {
    plain($(valueId), text);
    $(wordId).hidden = !text;
  };

  // Title/artist reach the preference panel when their own mbid is known
  // `[REQ-VIS-285]`, plain text otherwise -- unidentified audio, or an
  // uncredited artist, the same case that already leaves `artist` unset.
  const prefLink = (el, kind, id, text) => {
    if (!el) return;
    el.classList.toggle('pref-link', !!id);
    el.onclick = id ? () => Vaino.editPreference(kind, id, text) : null;
  };

  // Reads the same snapshot the display does, so a click can only act on
  // the track actually shown [REQ-VIS-225].
  let latest = null;
  Vaino.seekable($('bar'), () => latest);

  Vaino.subscribe(s => {
    latest = s;
    plain($('title'), s.title ?? '—');
    prefLink($('title'), 'recording', s.mbid, s.title);
    pair('byword', 'artist', s.artist);
    prefLink($('artist'), 'artist', s.artist_mbid, s.artist);
    pair('fromword', 'album', s.album);
    $('plays').textContent = Vaino.fmt.plays(s.plays, s.last_played);
    Vaino.showArt($('art'), s.passage_id);
    // The original showed the back of the sleeve beside the front when it
    // had one -- 559 of its 675 albums did. Hidden by itself when absent.
    Vaino.showBackArt($('artback'), s.passage_id);
    Vaino.showLyrics($('lyrics'), s.passage_id);
    $('time').textContent = `${clock(s.position_ms)} / ${clock(s.duration_ms)}`;
    $('fill').style.width =
      s.duration_ms ? `${(s.position_ms / s.duration_ms) * 100}%` : '0';
    $('bar').classList.toggle('seekable', Boolean(s.can_seek && s.duration_ms));
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
