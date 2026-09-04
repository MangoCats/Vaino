// The player, without a single opinion about how it looks [REQ-VIS-160].
//
// Everything a skin needs and nothing it should have to reinvent: the socket,
// the snapshot, the commands, and the two pieces of arithmetic that are
// specified rather than decorative. A skin supplies markup, a stylesheet, and a
// render function; it never opens a socket, never builds a URL, and never
// carries a copy of the fader curve.
//
// The split is not new -- it is the one the server already draws. `/ws` pushes
// complete snapshots and the POST endpoints take commands; that contract is the
// real interface, and the DOM was only ever one rendering of it.
const Vaino = (() => {
  const listeners = [];
  let last = null;

  // ---- commands ----------------------------------------------------------
  // Every one is fire-and-forget. The truth comes back in the next snapshot,
  // so nothing here updates the display optimistically and then has to explain
  // itself when the engine disagrees.
  const post = path => fetch(path, { method: 'POST' });

  // ---- the fader ---------------------------------------------------------
  // Quadratic in travel, flat where it meets full scale [REQ-AUD-156]. This
  // lives here, not in a skin: it is a specified control law, and three skins
  // each carrying their own copy is three chances to disagree with the engine.
  //
  // The floor comes from the engine, so the number exists in exactly one place.
  let faderFloor = -72;
  const fader = {
    // Travel (0 at the left, 1 at the right) to dB.
    db: x => faderFloor * (1 - x) ** 2,
    // dB back to travel, for putting the knob where the engine says it is.
    travel: db => 1 - Math.sqrt(db / faderFloor),
    get floor() { return faderFloor; },
  };

  // ---- formatting --------------------------------------------------------
  const fmt = {
    clock(ms) {
      const t = Math.max(0, Math.round(ms / 1000));
      return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`;
    },
    // Displayed to a tenth of a dB, and SENT as the displayed figure, so the
    // caption cannot differ from the level in force.
    round1: v => Math.round(v * 10) / 10,
    db: db => `${db <= -0.05 ? '−' + Math.abs(db).toFixed(1) : '0.0'} dB`,
    // "12 plays", and when. Play counts are per RECORDING, so the phrasing
    // must not imply this file: the same recording reached through two files
    // is the same thing heard twice.
    plays(n, at) {
      if (!n) return 'never played';
      const times = n === 1 ? 'once' : `${n} times`;
      return at ? `played ${times}, last ${new Date(at * 1000).toLocaleDateString()}`
                : `played ${times}`;
    },
    // "1.5 s of overlap" / "0.5 s of silence between" / "back to back".
    // A lead longer than the fade is legal and leaves a gap; say so plainly
    // rather than letting silence come as a surprise.
    overlap(k) {
      const s = (k.fade_ms - k.lead_ms) / 1000;
      return s > 0 ? `${s.toFixed(1)} s of overlap`
           : s < 0 ? `${(-s).toFixed(1)} s of silence between`
           : 'back to back';
    },
  };

  // ---- provenance --------------------------------------------------------
  // Where a displayed name came from `[REQ-VIS-120]`.
  //
  // "The MusicBrainz Recording title" and "whatever this file's ID3 tag says"
  // are different claims, and nothing in the name itself betrays which one you
  // are reading: both arrive as ordinary text, and one of them is a stem of a
  // filename with the underscores taken out. This library is a migration whose
  // every recording id came from one source, so the difference is not academic.
  //
  // Visible rather than on hover, which is where it used to be: a tooltip is
  // no use on the phone this interface is mostly read from, and a claim you
  // have to go looking for is one nobody checks.
  const SOURCE = {
    musicbrainz: { mark: 'MB', label: 'MusicBrainz' },
    tags: { mark: 'tag', label: 'the file tags' },
    filename: { mark: 'file', label: 'the filename' },
    unknown: { mark: '?', label: 'nowhere' },
  };

  function badge(source, what) {
    const s = SOURCE[source] || SOURCE.unknown;
    const b = document.createElement('span');
    b.className = 'src';
    b.dataset.src = source || 'unknown';
    b.textContent = s.mark;
    // The long form stays on hover for anyone who wants the sentence.
    b.title = `${what} from ${s.label}`;
    return b;
  }

  // Put `text` in `el` and hang its provenance off the end. No badge on an
  // empty name: there is no claim to qualify, and "unknown from nowhere" is
  // noise rather than information.
  function named(el, text, source, what) {
    if (!el) return;
    el.textContent = text ?? '';
    if (text) el.appendChild(badge(source, what));
  }

  // ---- cover art ---------------------------------------------------------
  // A URL and the load/error dance around it. The URL because core owns every
  // route; the dance because all three skins would otherwise carry the same
  // eight lines, and art is missing often enough -- roughly a third of this
  // library -- that getting the failure case wrong would be conspicuous.
  //
  // Nothing is asked of the server until a skin asks: a 404 is the normal
  // answer for a file with no embedded picture, not an error to report.
  // The back of the sleeve, for skins that show it. Separate route, same
  // dance: a passage with no back cover is the normal case, not an error, so
  // the element hides on 404 exactly as the front does.
  // The kantele `[REQ-VIS-128]`, shown where a passage has no cover.
  //
  // Väinö is Väinämöinen, and the kantele is his instrument; the project
  // already borrows from the same source in Sampo. Drawn from the instrument
  // rather than from a stock illustration of one, which is why the strings
  // terminate ON the varras -- the bar at the NARROW end that they are knotted
  // around -- and run to tuning pins at the wide end. A traditional five-string
  // kantele has no sound hole; the body is hollowed from beneath.
  //
  // Inline markup rather than a data URI, because a data URI is an isolated
  // document and cannot see `currentColor`. Inlined, one mark takes each skin's
  // own text colour: gold in MuLibPlay, LCD green in WinAmp, dim grey in Vaino.
  const KANTELE =
    '<svg viewBox="0 0 96 96" fill="none" stroke="currentColor" ' +
    'stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
    '<path d="M16 54 C31 48 48 40 79 32 L79 50 C53 54 33 58 16 54 Z" stroke-width="1.3" opacity=".34"/>' +
    '<path d="M22.1 51.3 C40 46.0 58 39.8 75 35.5" stroke-width=".95" opacity=".6"/>' +
    '<path d="M22.2 52.3 C40 47.5 58 41.8 75 38.5" stroke-width=".95" opacity=".55"/>' +
    '<path d="M22.3 53.3 C40 49.0 58 44.3 75 41.5" stroke-width=".95" opacity=".5"/>' +
    '<path d="M22.4 54.3 C40 50.6 58 46.8 75 44.5" stroke-width=".95" opacity=".45"/>' +
    '<path d="M22.5 55.3 C40 52.2 58 49.3 75 47.5" stroke-width=".95" opacity=".4"/>' +
    '<path d="M21.9 50.1 L22.6 56.5" stroke-width="1.6" opacity=".55"/>' +
    '<g fill="currentColor" stroke="none" opacity=".5">' +
    '<circle cx="76.4" cy="35.5" r="1.15"/><circle cx="76.4" cy="38.5" r="1.15"/>' +
    '<circle cx="76.4" cy="41.5" r="1.15"/><circle cx="76.4" cy="44.5" r="1.15"/>' +
    '<circle cx="76.4" cy="47.5" r="1.15"/></g></svg>';

  // Structure, not decoration, so it belongs to core rather than to each skin:
  // two stacked layers is the only way to fade BETWEEN two covers, since
  // swapping one element's `src` is instantaneous. Skins keep the sizing and
  // the object-fit; they do not have to know how the crossfade is built.
  const ART_CSS =
    '.artbox { position: relative; overflow: hidden; }' +
    '.artbox > img, .artbox > .ph { position: absolute; inset: 0;' +
    '  width: 100%; height: 100%; }' +
    '.artbox > img { opacity: 0; transition: opacity 1s ease; }' +
    '.artbox > img.on { opacity: 1; }' +
    '.artbox > .ph { display: grid; place-items: center;' +
    '  transition: opacity 1s ease; }' +
    '.artbox.has-art > .ph { opacity: 0; }' +
    '.artbox > .ph svg { width: 78%; height: 78%; }' +
    '@media (prefers-reduced-motion: reduce) {' +
    '  .artbox > img, .artbox > .ph { transition: none; } }';

  function artStyle() {
    if (document.getElementById('vaino-art-css')) return;
    const st = document.createElement('style');
    st.id = 'vaino-art-css';
    st.textContent = ART_CSS;
    document.head.appendChild(st);
  }

  // Build the two layers and the mark beneath them, once per box.
  function artBox(box) {
    if (box.dataset.art) return;
    artStyle();
    box.dataset.art = '1';
    box.classList.add('artbox');
    box.textContent = '';
    const ph = document.createElement('span');
    ph.className = 'ph';
    ph.innerHTML = KANTELE;
    const a = document.createElement('img');
    const b = document.createElement('img');
    a.alt = b.alt = '';
    box.append(ph, a, b);
  }

  function showBackArt(box, passageId) {
    return showArt(box, passageId, true);
  }

  // The words for `passageId`, fetched once per passage `[SPEC-LYR-040]`.
  //
  // Fetched rather than pushed: the snapshot goes out on every tick to every
  // skin, and a song's words change once a song. `lastLyrics` is what stops one
  // request per tick — the passage id is the whole cache key, because two
  // passages of one recording share their words anyway.
  //
  // A 404 is the ordinary answer for most of the library and empties the box
  // rather than showing an error: a panel saying "not found" for three tracks
  // out of four is worse than a panel that is simply not there.
  let lastLyrics = null;
  function showLyrics(box, passageId) {
    if (!box) return;
    if (passageId == null) { box.textContent = ''; box.hidden = true; lastLyrics = null; return; }
    if (passageId === lastLyrics) return;
    lastLyrics = passageId;
    fetch(`/lyrics/${passageId}`)
      .then(r => (r.ok ? r.text() : null))
      .then(t => {
        if (passageId !== lastLyrics) return;   // a later track won the race
        box.textContent = t || '';
        box.hidden = !t;
      })
      .catch(() => { box.textContent = ''; box.hidden = true; });
  }

  // Fade from whatever is showing to the cover for `passageId`.
  //
  // The box never empties: the mark sits under both layers, so the element
  // keeps its size whatever happens. That is what stopped the controls beneath
  // it jumping up and down as tracks changed -- the old version toggled
  // `hidden`, which is `display: none`, so the art left the layout entirely
  // while the next one loaded and the page reflowed twice per track
  // `[REQ-VIS-127]`.
  function showArt(box, passageId, back) {
    if (!box) return;
    artBox(box);
    const want = passageId == null ? ''
               : back ? `/art/${passageId}/back` : `/art/${passageId}`;
    if (box.dataset.for === want) return;   // already showing it
    box.dataset.for = want;

    const imgs = box.querySelectorAll('img');
    const clear = () => imgs.forEach(i => i.classList.remove('on'));
    if (!want) {                            // nothing playing: back to the mark
      clear();
      box.classList.remove('has-art');
      return;
    }
    const live = box.querySelector('img.on');
    const idle = live === imgs[0] ? imgs[1] : imgs[0];
    // Swap only once the incoming cover has decoded. Fading toward an image
    // that has not arrived shows an empty box for the length of the fade,
    // which is the artefact this exists to remove.
    idle.onload = () => {
      if (box.dataset.for !== want) return; // superseded while loading
      clear();
      idle.classList.add('on');
      box.classList.add('has-art');
    };
    // A 404 is the ordinary answer for a passage with no embedded picture --
    // roughly a third of this library -- so it fades to the mark rather than
    // being reported.
    idle.onerror = () => {
      if (box.dataset.for !== want) return;
      clear();
      box.classList.remove('has-art');
    };
    idle.src = want;
  }

  // The edit controls for one queued passage, wired and ready to append.
  //
  // Here rather than in each skin because all three want the same three
  // verbs on the same object, and three copies would drift. A skin styles
  // them through `.qedit` and decides where they go; it does not decide
  // what they do.
  // Takes the ENTRY's id, not the passage's `[REQ-VIS-186]`. A passage may be
  // queued twice; those are two entries naming one recording, and addressing
  // them by passage meant removing one removed both and moving one moved
  // whichever happened to come first.
  function queueControls(qid, editable = true) {
      const box = document.createElement('span');
      box.className = 'qedit';
      // Remove first and furthest left, then sooner, then later. Fixed
      // order and fixed widths so the controls line up as columns down the
      // queue: a column of identical buttons is one target to learn, where
      // buttons that shift with the length of a title are three.
      for (const [label, action, title] of Vaino.VERBS.edit) {
          const b = document.createElement('button');
          b.type = 'button';
          b.textContent = label;
          b.title = title;
          if (editable === false) {
              // Already in the mixer, so its audio is partly in the ring:
              // the control would report success and change nothing.
              b.disabled = true;
              b.title = 'already playing into the buffer';
          }
          b.onclick = e => {
              // The row itself may do something else entirely.
              e.stopPropagation();
              post(`/queue/${qid}/${action}`);
          };
          box.appendChild(b);
      }
      return box;
  }

  // ---- binders -----------------------------------------------------------
  // Behaviour every skin needs and none should own. A skin marks up a slider,
  // a select and a list; these make them work, and the skin keeps only what it
  // is for -- where things go and what they look like.
  //
  // Each returns a render function for the snapshot, which `subscribe` calls.
  // They are opt-in: a skin that wants to do something unusual simply does not
  // call them, and nothing here is load-bearing for the rest.

  // The volume slider. While dragging, the slider is the truth -- adopting the
  // pushed value mid-drag would fight the user's thumb twice a second -- and
  // the level is sent on release, not per pixel of travel.
  function bindVolume(slider, label) {
    let holding = false;
    let pending = 0;
    const show = db => { if (label) label.textContent = fmt.db(db); };
    slider.oninput = () => {
      holding = true;
      pending = fmt.round1(fader.db(Number(slider.value)));
      show(pending);
    };
    slider.onchange = () => { holding = false; post(`/volume/${pending}`); };
    return s => {
      if (holding) return;
      const db = fmt.round1(s.volume_db ?? 0);
      slider.value = fader.travel(db);
      show(db);
    };
  }

  // The programme picker. The option list is rebuilt only when it actually
  // changes; replacing it on every push would close the dropdown in the hand
  // of whoever is reading it.
  function bindProgram(select, autoLabel = 'Automatic (by time of day)') {
    let signature = '';
    select.onchange = e => post(`/program/${e.target.value}`);
    return s => {
      const programs = s.programs || [];
      const sig = programs.map(p => p.id + p.name).join('|');
      if (sig !== signature) {
        signature = sig;
        select.textContent = '';
        const auto = document.createElement('option');
        auto.value = 'auto';
        auto.textContent = autoLabel;
        select.appendChild(auto);
        for (const p of programs) {
          const o = document.createElement('option');
          o.value = p.id;
          o.textContent = `${p.name} — from ${p.start}`;
          select.appendChild(o);
        }
      }
      if (!s.program_manual) {
        select.value = 'auto';
      } else {
        const m = programs.find(p => p.name === s.program);
        if (m) select.value = String(m.id);
      }
    };
  }

  // One queue row: edit controls first, then the title, then the duration.
  // The skin supplies the element it wants and how the row should read; the
  // order and the controls are the same everywhere because they are the same
  // idea everywhere `[REQ-VIS-185]`.
  function queueRow(item, label, tag = 'li', opts = {}) {
    const row = document.createElement(tag);
    row.dataset.passage = item.passage_id;
    row.dataset.qid = item.qid;
    if (item.editable === false) row.dataset.locked = '1';
    // A skin may put ONE set of controls beside a selected row instead of a
    // set on every row. Both are honest arrangements; which suits depends on
    // whether the skin has a notion of a selected track.
    if (opts.controls !== false) {
      row.appendChild(queueControls(item.qid, item.editable));
    }
    const title = document.createElement('span');
    title.className = 'qtitle';
    // `opts.linkable`: title/artist become their own clickable spans,
    // reaching the preference panel `[REQ-VIS-285]`, instead of one plain
    // text node -- a skin opts in because its own `label()` string is no
    // longer what gets shown. Skins that don't ask stay byte-for-byte
    // unchanged (WinAmp's marquee has no separate artist node to link).
    if (opts.linkable) {
      linkableTrack(title, item);
    } else {
      title.appendChild(document.createTextNode(label(item) + ' '));
    }
    const dur = document.createElement('span');
    dur.className = 'dur';
    dur.textContent = fmt.clock(item.duration_ms);
    title.appendChild(dur);
    row.appendChild(title);
    return row;
  }

  // The whole list, including the empty case, which every skin got wrong in
  // its own way before this.
  function bindQueue(container, label, tag = 'li', empty = 'nothing queued', opts = {}) {
    return s => {
      container.textContent = '';
      const items = s.queue || [];
      if (!items.length) {
        const none = document.createElement(tag);
        none.className = 'empty';
        none.textContent = empty;
        container.appendChild(none);
        return;
      }
      // Read per render, not per bind: the rows are rebuilt on every snapshot
      // and the selection changes between them, so a value captured when the
      // binder was made would mark the wrong row for ever. A function is
      // accepted for exactly that reason.
      const sel = typeof opts.selected === 'function' ? opts.selected() : opts.selected;
      for (const q of items) {
        const row = queueRow(q, label, tag, opts);
        // Selection is the skin's idea; the row only reports the click and
        // wears the mark the skin asks for.
        // The entry, so a skin can tell two copies of one passage apart; the
        // whole item too, because what a skin wants next is usually the
        // passage behind it -- an explanation is about the recording, not
        // about which copy of it is queued.
        if (opts.onPick) row.onclick = () => opts.onPick(q.qid, q);
        if (sel != null && sel === q.qid) row.classList.add('picked');
        container.appendChild(row);
      }
    };
  }

  // ---- preference panel [REQ-VIS-285], [REQ-VIS-290] ---------------------
  // MuLibPlay let a listener hand-tune an artist's or a recording's own
  // rotation/recovery/restraint directly. Vaino's schema carries those
  // exact values forward (`listener_preferences`) but nothing reached them
  // until this -- one shared panel, built once and reused, rather than a
  // bespoke editor per skin: there is no modal/dialog convention anywhere
  // in this codebase, and MuLibPlay's own skin has no panel-switching
  // machinery to piggyback on the way Vaino's skin does.

  // Builds `<title-span>[ — <artist-span>]`, each a clickable link when its
  // own mbid is known, plain text otherwise (unidentified audio, or a
  // recording with no linked artist -- the same case that already leaves
  // `artist` unset). Shared by every skin that opts into `queueRow`'s
  // `linkable` mode and by each skin's own now-playing render.
  function linkableTrack(container, item, opts = {}) {
    const open = (kind, id, label, e) => {
      e.stopPropagation();
      e.preventDefault();
      Vaino.editPreference(kind, id, label);
    };
    const titleEl = document.createElement(item.mbid ? 'a' : 'span');
    titleEl.className = 'pref-link pref-link-title';
    titleEl.textContent = item.title ?? '';
    if (item.mbid) {
      titleEl.href = '#';
      titleEl.onclick = e => open('recording', item.mbid, item.title, e);
    }
    container.appendChild(titleEl);
    if (item.artist) {
      container.appendChild(document.createTextNode(opts.artistJoin ?? ' — '));
      const artistEl = document.createElement(item.artist_mbid ? 'a' : 'span');
      artistEl.className = 'pref-link pref-link-artist';
      artistEl.textContent = item.artist;
      if (item.artist_mbid) {
        artistEl.href = '#';
        artistEl.onclick = e => open('artist', item.artist_mbid, item.artist, e);
      }
      container.appendChild(artistEl);
    }
    container.appendChild(document.createTextNode(' '));
  }

  // Base styling, injected once, so the panel is usable the moment a skin
  // adopts it -- a skin's own CSS may still restyle `.pref-panel`/`.pref-box`
  // to match its look, the same "each skin's own CSS can restyle it" note
  // this whole panel was built under.
  const PREF_CSS = `
    .pref-link { cursor: pointer; text-decoration: underline dotted; text-underline-offset: 2px; }
    .pref-panel { position: fixed; inset: 0; z-index: 1000; background: rgba(0,0,0,.5);
      display: flex; align-items: center; justify-content: center; }
    .pref-box { background: #fff; color: #111; border-radius: 8px; padding: 1.25em 1.5em;
      min-width: 20em; max-width: 90vw; box-shadow: 0 4px 24px rgba(0,0,0,.4); }
    .pref-heading { margin: 0 0 .75em; font-size: 1.1em; }
    .pref-field { display: grid; grid-template-columns: 5.5em 1fr auto auto; align-items: center;
      gap: .5em; margin: .5em 0; }
    .pref-field input[type=range] { width: 12em; }
    .pref-readout { font-size: .9em; opacity: .8; min-width: 8em; }
    .pref-error { color: #b00020; }
    .pref-actions { margin-top: 1em; text-align: right; }
    .pref-actions button { margin-left: .5em; }
  `;

  // The panel itself: built once, lazily, appended to `document.body` --
  // reused by every call, across every skin that uses it, so there is
  // exactly one implementation to style and to get right.
  let prefPanel = null;
  function ensurePrefPanel() {
    if (prefPanel) return prefPanel;
    const style = document.createElement('style');
    style.id = 'pref-panel-base-style';
    style.textContent = PREF_CSS;
    document.head.appendChild(style);
    const div = document.createElement('div');
    div.id = 'pref-panel';
    div.className = 'pref-panel';
    div.hidden = true;
    div.innerHTML = `
      <div class="pref-box" role="dialog" aria-modal="true">
        <h2 class="pref-heading"></h2>
        <p class="pref-error" hidden></p>
        <div class="pref-field" data-field="rotation">
          <label>Cooldown</label>
          <input type="range" min="-1" max="2" step="0.01">
          <span class="pref-readout"></span>
          <button type="button" class="pref-reset" title="use the default">Reset</button>
        </div>
        <div class="pref-field" data-field="recovery">
          <label>Recovery</label>
          <input type="range" min="-1" max="2" step="0.01">
          <span class="pref-readout"></span>
          <button type="button" class="pref-reset" title="use the default">Reset</button>
        </div>
        <div class="pref-field" data-field="restraint">
          <label>Preference</label>
          <input type="range" min="-2" max="5" step="0.01">
          <span class="pref-readout"></span>
          <button type="button" class="pref-reset" title="use the default">Reset</button>
        </div>
        <div class="pref-actions">
          <button type="button" class="pref-cancel">Cancel</button>
          <button type="button" class="pref-save">Save</button>
        </div>
      </div>`;
    document.body.appendChild(div);
    // A click on the backdrop closes it; a click inside the box must not
    // bubble to that same handler.
    div.addEventListener('click', e => { if (e.target === div) closePrefPanel(); });
    div.querySelector('.pref-box').addEventListener('click', e => e.stopPropagation());
    div.querySelector('.pref-cancel').onclick = closePrefPanel;
    prefPanel = div;
    return div;
  }

  function closePrefPanel() {
    if (prefPanel) prefPanel.hidden = true;
  }

  // Rotation/recovery are log-scale hours `[SPEC-DIR-110]`: `10^v` hours.
  // Restraint is a log-scale multiplier: `10^-v`. Both formulas are
  // `frequency.rs`'s own -- this is a *readout*, not a second definition;
  // the server is still the one place a value actually takes effect.
  function prefDuration(v) {
    const hours = 10 ** v;
    if (hours < 1) return `≈ ${Math.round(hours * 60)} min`;
    if (hours < 48) return `≈ ${hours.toFixed(1)} hrs`;
    return `≈ ${(hours / 24).toFixed(1)} days`;
  }
  function prefMultiplier(v) {
    const m = 10 ** -v;
    return m >= 1 ? `≈ ${m.toFixed(1)}× as often` : `≈ ${m.toFixed(2)}× as often`;
  }
  const READOUT = { rotation: prefDuration, recovery: prefDuration, restraint: prefMultiplier };

  // Opens the panel for one subject, fetches its current tuning, and wires
  // Save/Reset. `kind` is `'recording'` or `'artist'`; `id` its mbid.
  // A subject with no mbid (unidentified audio, an uncredited artist) has
  // nothing to open -- callers only invoke this when an id is present.
  async function editPreference(kind, id, label) {
    const panel = ensurePrefPanel();
    const err = panel.querySelector('.pref-error');
    err.hidden = true;
    panel.querySelector('.pref-heading').textContent =
      `${kind === 'artist' ? 'Artist' : 'Recording'} preferences — ${label ?? id}`;
    panel.hidden = false;

    let current = {};
    let defaults = {};
    try {
      const r = await fetch(`/preference/${kind}/${encodeURIComponent(id)}`);
      if (!r.ok) throw new Error(`the server answered ${r.status}`);
      const doc = await r.json();
      current = doc;
      defaults = doc.defaults;
    } catch (e) {
      err.textContent = `could not load current preferences: ${e.message}`;
      err.hidden = false;
      return;
    }

    // Per-field state: `null` means "at the default", a number means
    // "explicitly set to this" -- the same three-way shape the server
    // itself keeps, so Save only ever sends what actually changed.
    const state = {};
    const cleared = new Set();
    for (const field of ['rotation', 'recovery', 'restraint']) {
      state[field] = current[field] ?? defaults[field];
      const box = panel.querySelector(`.pref-field[data-field="${field}"]`);
      const input = box.querySelector('input');
      const readout = box.querySelector('.pref-readout');
      const resetBtn = box.querySelector('.pref-reset');
      const refresh = () => {
        readout.textContent = READOUT[field](state[field])
          + (current[field] == null ? ' (default)' : '');
      };
      input.value = state[field];
      refresh();
      input.oninput = () => {
        state[field] = Number(input.value);
        cleared.delete(field);
        current[field] = state[field]; // no longer "at the default" once dragged
        refresh();
      };
      resetBtn.onclick = () => {
        state[field] = defaults[field];
        current[field] = null;
        cleared.add(field);
        input.value = state[field];
        refresh();
      };
    }

    panel.querySelector('.pref-save').onclick = async () => {
      const q = new URLSearchParams();
      for (const field of ['rotation', 'recovery', 'restraint']) {
        if (cleared.has(field)) q.set(field, '');
        else if (current[field] != null) q.set(field, String(state[field]));
      }
      try {
        const r = await fetch(`/preference/${kind}/${encodeURIComponent(id)}?${q}`,
          { method: 'POST' });
        if (!r.ok) throw new Error(`the server answered ${r.status}`);
        closePrefPanel();
      } catch (e) {
        err.textContent = `could not save: ${e.message}`;
        err.hidden = false;
      }
    };
  }

  // ---- skins -------------------------------------------------------------
  // The choice is per browser, not per player: two people on two phones may
  // want different skins of the same radio, and neither should be able to
  // restyle the other. That is why it lives in localStorage and not the engine.
  const KEY = 'vaino.skin';
  let catalogue = [];

  // MuLibPlay is what a browser that has never chosen gets `[REQ-VIS-124]`:
  // the face six years of listening happened in front of. A browser that HAS
  // chosen keeps its choice, which is the whole reason this is stored.
  const DEFAULT_SKIN = 'mulibplay';

  // localStorage rather than a cookie, deliberately. The server never needs to
  // know which skin a browser wears -- the shell fetches it -- so a cookie
  // would ride on every request, including the WebSocket upgrade and every art
  // fetch, to tell the server something it does not use. This also survives
  // where a session cookie would not.
  //
  // Wrapped because storage THROWS rather than returning null when a browser
  // is in a private mode or has site data blocked. Unwrapped, that exception
  // lands before any skin loads, and the page is blank rather than merely
  // unable to remember.
  function remember(name) {
    try { localStorage.setItem(KEY, name); } catch { /* unremembered, not broken */ }
  }
  function remembered() {
    try { return localStorage.getItem(KEY); } catch { return null; }
  }

  function chosen() {
    const q = new URLSearchParams(location.search).get('skin');
    if (q) remember(q);
    return q || remembered() || DEFAULT_SKIN;
  }

  function setSkin(name) {
    remember(name);
    location.href = location.pathname; // drop ?skin= so it is not sticky twice
  }

  // A skin is markup, a stylesheet and a script, in that order: the script may
  // assume its own DOM is present, which is the whole reason it loads last.
  async function loadSkin(name) {
    const base = `/skin/${name}`;
    const [html] = await Promise.all([
      fetch(`${base}/skin.html`).then(r => r.text()),
      new Promise((ok, no) => {
        const l = document.createElement('link');
        l.rel = 'stylesheet';
        l.href = `${base}/skin.css`;
        l.onload = ok;
        l.onerror = no;
        document.head.appendChild(l);
      }),
    ]);
    document.getElementById('app').innerHTML = html;
    // Buttons are wired centrally so no skin has to know a URL to be playable.
    for (const b of document.querySelectorAll('[data-cmd]')) {
      b.onclick = () => post(`/command/${b.dataset.cmd}`);
    }
    for (const b of document.querySelectorAll('[data-skin]')) {
      b.onclick = () => setSkin(b.dataset.skin);
    }
    // A skin marks up an empty <select data-skins> and gets a working picker;
    // the catalogue comes from the server, so adding a skin never means editing
    // three others to list it.
    for (const sel of document.querySelectorAll('[data-skins]')) {
      sel.textContent = '';
      for (const s of catalogue) {
        const o = document.createElement('option');
        o.value = s.name;
        o.textContent = s.label;
        sel.appendChild(o);
      }
      sel.value = chosen();
      sel.onchange = () => setSkin(sel.value);
    }
    await new Promise((ok, no) => {
      const s = document.createElement('script');
      s.src = `${base}/skin.js`;
      s.onload = ok;
      s.onerror = no;
      document.body.appendChild(s);
    });
    // A skin that loads after the first snapshot must not sit blank waiting for
    // the next one, which may be half a second away.
    if (last) dispatch(last);
  }

  function dispatch(s) {
    last = s;
    if (s.fader_min_db) faderFloor = s.fader_min_db;
    for (const fn of listeners) {
      // One skin's mistake must not silence the rest of the page, and a render
      // that throws every 500 ms would otherwise fill the console and stop.
      try { fn(s); } catch (e) { console.error('skin render failed', e); }
    }
  }

  // ---- the socket --------------------------------------------------------
  // Reconnect rather than go stale: the player outlives any one page load, and
  // a silently dead socket looks exactly like paused playback.
  let status = 'connecting';
  function connect() {
    const ws = new WebSocket(`ws://${location.host}/ws`);
    ws.onmessage = e => dispatch(JSON.parse(e.data));
    ws.onopen = () => { status = 'connected'; };
    ws.onclose = () => {
      status = 'reconnecting';
      if (last) dispatch(last); // let skins show the disconnection
      setTimeout(connect, 1000);
    };
  }

  return {
    subscribe(fn) { listeners.push(fn); if (last) fn(last); },
    get snapshot() { return last; },
    get status() { return status; },
    get skins() { return catalogue; },
    get skin() { return chosen(); },
    setSkin,
    fader,
    fmt,
    bindVolume,
    bindProgram,
    bindQueue,
    queueRow,
    showArt,
    showLyrics,
    artUrl: id => `/art/${id}`,
    command: name => post(`/command/${name}`),
    volume: db => post(`/volume/${db}`),
    seek: ms => post(`/seek/${Math.max(0, Math.round(ms))}`),
    restartUnderruns: () => post('/underruns/restart'),
    // A unix second to something a person reads. Local time, because the
    // listener is standing in it.
    since(unix) {
      if (!unix) return '';
      const d = new Date(unix * 1000);
      const today = new Date();
      const sameDay = d.toDateString() === today.toDateString();
      const clock = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
      return sameDay ? clock : `${d.toLocaleDateString()} ${clock}`;
    },
    // Wire a progress bar up as a control. Shared because both skins want
    // the same arithmetic, and a second copy of it would drift.
    seekable(bar, state) {
      bar.addEventListener('click', e => {
        const s = state();
        if (!s || !s.can_seek || !s.duration_ms) return;
        const box = bar.getBoundingClientRect();
        if (box.width <= 0) return;
        // Clamped: a click on the very edge of a padded hit area can land
        // fractionally outside the bar itself.
        const at = Math.min(Math.max((e.clientX - box.left) / box.width, 0), 1);
        Vaino.seek(at * s.duration_ms);
      });
    },
    program: id => post(`/program/${id}`),
    skipFade: ms => post(`/skip/fade/${Math.round(ms)}`),
    skipLead: ms => post(`/skip/lead/${Math.round(ms)}`),
    resumeSave: ms => post(`/resume/save/${Math.round(ms)}`),
    skipSuppress: h => post(`/skip/suppress/${Math.round(h)}`),
    dequeueSuppress: h => post(`/dequeue/suppress/${Math.round(h)}`),
    queueDepth: n => post(`/queue/depth/${Math.round(n)}`),
    sampleInterval: ms => post(`/sample/interval/${Math.round(ms)}`),
    switchBackend: which => post(`/backend/${which}`),
    cueSheets: on => post(`/cue/${on ? 'on' : 'off'}`),
    covers: on => post(`/covers/${on ? 'on' : 'off'}`),
    lyricsCache: on => post(`/lyricscache/${on ? 'on' : 'off'}`),
    lyricsSidecar: on => post(`/lyricssidecar/${on ? 'on' : 'off'}`),
    // The player page: load the chosen skin, then follow the socket.
    async start() {
      catalogue = await fetch('/skins').then(r => r.json()).catch(() => []);
      await loadSkin(chosen());
      connect();
    },
    // The browse page: it wants the skin's LOOK and the command helpers, but
    // not the player's markup and not a socket -- a library listing does not
    // change twice a second.
    async startBare() {
      catalogue = await fetch('/skins').then(r => r.json()).catch(() => []);
      const l = document.createElement('link');
      l.rel = 'stylesheet';
      l.href = `/skin/${chosen()}/skin.css`;
      document.head.appendChild(l);
    },
    // Throws rather than resolving to nothing when the query fails, so a
    // broken listing cannot be mistaken for an empty library.
    browse: (kind, filter = {}) => {
      const q = new URLSearchParams();
      for (const [k, v] of Object.entries(filter)) if (v) q.set(k, v);
      return fetch(`/browse/${kind}?${q}`).then(r => {
        if (!r.ok) throw new Error(`the server answered ${r.status}`);
        return r.json();
      });
    },
    // A page of what has actually sounded `[REQ-VIS-250]`. Same shape of
    // failure as `browse`: thrown, not silently empty.
    history: (page, size) =>
      fetch(`/history?page=${page}&size=${size}`).then(r => {
        if (!r.ok) throw new Error(`the server answered ${r.status}`);
        return r.json();
      }),
    // "Flag this for review" `[REQ-VIS-265]` -- on at any time, off at any
    // time, the same call either way with the new state carried as the query.
    setFlag: (kind, id, flagged) =>
      fetch(`/history/flag/${kind}/${encodeURIComponent(id)}?flagged=${flagged}`,
            { method: 'POST' }).then(r => {
        if (!r.ok) throw new Error(`the server answered ${r.status}`);
      }),
    // The verbs, in one place `[REQ-VIS-185]`. The engine validates them and
    // is the authority; this is the list the pages build their controls from,
    // so adding one means touching the match in `web.rs` and this array,
    // rather than three separate button-building loops.
    VERBS: {
        place: [
            ['Now', 'now', 'playing now'],
            ['Next', 'next', 'playing next'],
            ['Last', 'last', 'added to the end'],
        ],
        edit: [
            ['\u00d7', 'remove', 'remove from the queue'],
            ['\u2191', 'sooner', 'play sooner'],
            ['\u2193', 'later', 'play later'],
        ],
    },
    queue: (id, action) => post(`/queue/${id}/${action}`),

    queueControls,
    named,
    badge,
    showBackArt,
    editPreference,
  };
})();
