// Render every skin, in a real DOM, against snapshots the server could send.
//
// A skin is HTML, CSS and JavaScript, so `cargo test` cannot reach it: the Rust
// suite proves the SHELL serves the right bytes and stops there. This proves the
// bytes work -- that each skin loads through `core.js`'s own loader, survives a
// snapshot with everything in it and one with almost nothing, wires its
// transport, and drives volume through the shared fader curve rather than a
// private copy of it `[REQ-VIS-160]`.
//
// Optional by design: it needs node and jsdom, which the player itself does not.
// `verify-targets.sh` skips it when they are absent rather than failing, because
// a Pi has no business installing a JavaScript test runner to play music.
//
//   npm install jsdom && node build/verify-skins.js [snapshot.json]

const fs = require('fs');
const path = require('path');
// Exit 2, not 0, when the test cannot run. A harness that reports success
// because it did nothing is worse than no harness: this project has already
// been caught once committing on a verification that had quietly skipped.
let JSDOM;
try {
  ({ JSDOM } = require('jsdom'));
} catch {
  console.log('SKIPPED: jsdom is not installed (npm install jsdom)');
  process.exit(2);
}

const ROOT = path.join(__dirname, '..', 'player', 'src', 'web');

// An optional live capture, for checking a skin against real library data:
//   node build/verify-skins.js snapshot.json
// The built-in fixtures below are what actually gates the check, so that it
// needs no running player and no library.
const LIVE = process.argv[2]
  ? JSON.parse(fs.readFileSync(process.argv[2], 'utf8'))
  : null;

// A second snapshot exercising branches a healthy player may not reach:
// nothing playing, nothing queued, no programme, no explanation, fade of zero.
const SPARSE = {
  playing: false, title: null, position_ms: 0, duration_ms: 0,
  // Nothing named, nothing counted: the branch where every connecting word and
  // the cover must disappear rather than dangle [REQ-VIS-170].
  passage_id: null, artist: null, album: null, plays: 0, last_played: null,
  title_source: 'unknown', artist_source: 'unknown', album_source: 'unknown',
  queue_len: 0, queue: [], volume_db: -72.0, fader_min_db: -72.0,
  program: null, program_manual: false, programs: [],
  skip: { fade_ms: 0, lead_ms: 500, fade_max_ms: 10000, lead_min_ms: 100, lead_max_ms: 2000,
          resume_save_ms: 5000, resume_save_min_ms: 1000, resume_save_max_ms: 300000 },
  underrun_samples: 0, why: null, dev_mode: false,
};

// A snapshot carrying a full Program Director explanation -- the richest
// branch, and the one a live capture may not happen to include.
const RICH = {
  ...SPARSE,
  playing: true, title: 'A Passage With Reasons', position_ms: 61000, duration_ms: 244000,
  // MusicBrainz for the recording and the artist, the file's tag for the album
  // -- which is exactly the mixed provenance the live library produces today.
  passage_id: 4242, artist: 'Some Performer', album: 'Some Release',
  plays: 12, last_played: 1735689600,
  title_source: 'musicbrainz', artist_source: 'musicbrainz', album_source: 'tags',
  queue_len: 3,
  queue: [{ passage_id: 1, title: 'Next One', artist: 'Another', duration_ms: 180000 },
          { passage_id: 2, title: 'The One After', artist: null, duration_ms: 205000 }],
  volume_db: -12.5,
  program: 'Mellow', program_manual: true,
  programs: [{ id: 1, name: 'Mellow', start: '20:00' }, { id: 2, name: 'Prog', start: '09:00' }],
  skip: { fade_ms: 2000, lead_ms: 500, fade_max_ms: 10000, lead_min_ms: 100, lead_max_ms: 2000,
          resume_save_ms: 5000, resume_save_min_ms: 1000, resume_save_max_ms: 300000 },
  why: {
    program: 'Mellow', weight: 0.8123, decayed_weight: 0.4211, pool_weight: 3120.5,
    pool_size: 8078, share_pct: 0.0135,
    artist_weight: 1.0, track_restraint: 0.75, track_ramp: 1.0,
    related_damping: 0.9, length_bonus: 1.08, occasion: 1.0,
    shaping: { bypassed: false, eligible_in: 8078, gathered: 400, seeds_used: 3, disliked_out: 12 },
    seed_distances: [0.21, 0.33, 0.41], flow_distance: 0.187, rank: 4,
    runners_up: [{ title: 'Nearly', weight: 0.41 }, { title: 'Also Nearly', weight: 0.39 }],
    stages: 'frequency → shaping → flow',
  },
};

const skins = fs.readdirSync(path.join(ROOT, 'skins'));
let failures = 0;

async function run(skin) {
  const dir = path.join(ROOT, 'skins', skin);
  // The shell starts core with an inline script. Here core is injected by hand
  // (jsdom fetches no subresources), so that call would fire before core exists
  // and print a ReferenceError that means nothing. Drop it; we call start below.
  const shell = fs.readFileSync(path.join(ROOT, 'shell.html'), 'utf8')
    .replace('<script>Vaino.start();</script>', '');
  const dom = new JSDOM(shell,
                        { runScripts: 'dangerously', url: 'http://localhost/?skin=' + skin });
  const { window } = dom;
  const errors = [];
  window.console.error = (...a) => errors.push(a.join(' '));
  const check = (cond, msg) => { if (!cond) errors.push(msg); };

  // jsdom does not fetch subresources, so <link>/<script> onload would never
  // fire and core's loader would wait forever. Fire them; we run skin.js below.
  const create = window.document.createElement.bind(window.document);
  window.document.createElement = tag => {
    const el = create(tag);
    if (tag === 'link' || tag === 'script') setTimeout(() => el.onload && el.onload(), 0);
    return el;
  };

  const posted = [];
  window.fetch = (url, opts) => {
    if (opts && opts.method === 'POST') { posted.push(url); return Promise.resolve({ ok: true }); }
    if (url === '/skins') {
      return Promise.resolve({ json: () => Promise.resolve(skins.map(n => ({ name: n, label: n }))) });
    }
    const m = /^\/skin\/([^/]+)\/(.+)$/.exec(url);
    if (m) return Promise.resolve({ text: () => Promise.resolve(fs.readFileSync(path.join(ROOT, 'skins', m[1], m[2]), 'utf8')) });
    if (/^\/art\//.test(url)) return Promise.reject(new Error('no art in a test DOM'));
    // An explanation for a queued passage, so the panel's fetch path is
    // exercised rather than only its failure path.
    if (/^\/why\/\d+$/.test(url)) {
      return Promise.resolve({ ok: true, json: () => Promise.resolve(
        { ...RICH.why, program: 'Queued Reasons' }) });
    }
    return Promise.reject(new Error('unexpected fetch ' + url));
  };

  let sock = null;
  window.WebSocket = function () { sock = this; this.close = () => {}; };

  // As a <script>, not eval: a classic script's top-level `const` lands in the
  // shared script scope where later scripts can see it, and eval's does not.
  const runScript = src => {
    const el = create('script');
    el.textContent = src;
    window.document.body.appendChild(el);
  };
  runScript(fs.readFileSync(path.join(ROOT, 'core.js'), 'utf8'));
  // core.js is a library now; the page starts it, exactly as shell.html does.
  runScript('Vaino.start();');

  // Wait for core to have loaded the skin and opened its socket.
  for (let i = 0; i < 200 && !sock; i++) await new Promise(r => setTimeout(r, 5));
  if (!sock) { console.log(`${skin.padEnd(11)} FAIL  core never finished loading the skin`); failures++; return; }

  // core appended a <script> jsdom will not execute; run it in the same window.
  try {
    runScript(fs.readFileSync(path.join(dir, 'skin.js'), 'utf8'));
  } catch (e) {
    console.log(`${skin.padEnd(11)} FAIL  skin.js threw on load: ${e.message}`);
    failures++;
    return;
  }

  const cases = [['sparse', SPARSE], ['rich', RICH]];
  if (LIVE) cases.push(['live', LIVE]);
  for (const [label, snap] of cases) {
    sock.onopen && sock.onopen();
    try {
      sock.onmessage({ data: JSON.stringify(snap) });
    } catch (e) {
      errors.push(`${label} snapshot threw: ${e.message}`);
    }
  }

  // Cover art: jsdom never loads images, so the browser's load event has to be
  // faked. Without this the check would pass on a skin that can never show a
  // cover -- which is exactly the failure being guarded against.
  const art = window.document.getElementById('art');
  let artOk = 'no #art element';
  if (art) {
    if (!art.getAttribute('src')) {
      artOk = 'src never set';
    } else if (!art.hidden) {
      artOk = 'visible before it loaded';
    } else {
      art.dispatchEvent(new window.Event('load'));
      artOk = art.hidden ? 'still hidden after load' : null;
      if (!artOk) {
        // ...and moving to a passage whose file has no picture must hide it
        // again. Driven by pushing a snapshot, not by calling core directly,
        // so what is under test is the path the skin actually takes.
        sock.onmessage({ data: JSON.stringify({ ...RICH, passage_id: 999 }) });
        art.dispatchEvent(new window.Event('error'));
        artOk = art.hidden ? null : 'still visible after a 404';
      }
    }
  }
  if (artOk) errors.push('cover art: ' + artOk);

  // A skin that shows the back of the sleeve must ask the back route for it.
  // MuLibPlay put front and back side by side and 559 of its 675 albums had a
  // back; pointing both <img> at the same URL would show the front twice and
  // look deliberate.
  const back = window.document.getElementById('artback');
  if (back) {
    const front = art && art.getAttribute('src');
    const bsrc = back.getAttribute('src');
    check(/\/art\/\d+\/back$/.test(bsrc || ''),
          `back cover should come from the back route, got ${bsrc}`);
    check(bsrc !== front, 'front and back must not be the same image');
    // Absent is the common case, so it must hide on 404 rather than show a
    // broken image beside a good cover.
    back.dispatchEvent(new window.Event('error'));
    check(back.hidden, 'a missing back cover must hide, not render broken');
  }

  // The transport must be wired, and the picker populated from the catalogue.
  window.document.querySelector('[data-cmd="skip"]').onclick();
  // Volume must round-trip through the shared fader curve, not a per-skin copy.
  const vol = window.document.getElementById('volume');
  vol.value = 0.5;
  vol.oninput();
  vol.onchange();
  const picker = window.document.querySelector('[data-skins]');
  const opts = picker ? picker.options.length : 0;

  const title = window.document.getElementById('title');
  const queue = window.document.getElementById('queue');

  // Provenance must be VISIBLE, not a tooltip `[REQ-VIS-120]` -- that is the
  // whole point of the change, and a badge that renders nowhere would still
  // leave every other assertion here passing. The fixture names a MusicBrainz
  // title against a tag-sourced album, so a skin that hard-codes one marker
  // for the whole snapshot fails: at least two different sources must show.
  const badges = [...window.document.querySelectorAll('.src')];
  check(badges.length > 0, 'no provenance badge rendered anywhere');
  check(badges.every(b => b.textContent.trim()),
        'a provenance badge rendered empty');
  check(badges.every(b => b.title && /from /.test(b.title)),
        'a provenance badge carries no explanation on hover');
  check(badges.some(b => b.dataset.src === 'musicbrainz'),
        'the MusicBrainz-sourced name is not marked as one');
  // The badge must not be mistakeable for part of the name: it is a separate
  // element, so anything wanting the bare name can still get it.
  if (title) {
    check([...title.childNodes].some(n => n.nodeType === 3 &&
          n.textContent.includes('A Passage With Reasons')),
          'the title text is not a text node of its own');
  }

  // A skin that moves settings behind a gear must be able to open it again.
  // The controls stay in the DOM either way -- the bindings attach once at
  // load -- so the check is that the panels actually swap.
  const gear = window.document.getElementById('gear');
  if (gear) {
    const main = window.document.getElementById('panel-main');
    const set = window.document.getElementById('panel-settings');
    check(main && set, 'a gear needs both panels to switch between');
    check(!main.hidden && set.hidden, 'settings must start closed');
    gear.onclick();
    check(main.hidden && !set.hidden, 'the gear must open the settings screen');
    check(gear.getAttribute('aria-expanded') === 'true', 'aria-expanded must follow');
    // The controls moved there must still be wired to the engine.
    const fade = window.document.getElementById('skipfade');
    check(fade && typeof fade.onchange === 'function',
          'the skip control must still be bound after the move');
    fade.value = 3;
    fade.onchange();
    // The resume interval is bound in the same panel and must reach the engine.
    const rs = window.document.getElementById('resumesave');
    if (rs) {
      check(rs.value === '5.0', `resume interval should show 5.0 s, got ${rs.value}`);
      rs.value = 30; rs.onchange();
      check(posted.includes('/resume/save/30000'),
            `resume interval must post, got ${JSON.stringify(posted)}`);
    }
    check(posted.includes('/skip/fade/3000'),
          `a moved control must still reach the engine, posted ${JSON.stringify(posted)}`);
    gear.onclick();
    check(!main.hidden && set.hidden, 'the gear must close it again');
  }

  // Picking which track the explanation panel describes `[REQ-VIS-100]`.
  // The skin that does this shows ONE control set beside the picked row, so
  // the rows must not carry their own -- two ways to move a track is how they
  // drift apart.
  const nowrow = window.document.getElementById('nowrow');
  if (nowrow) {
    const rows = () => [...window.document.querySelectorAll('#queue li')];
    check(nowrow.classList.contains('picked'),
          'the playing track must be picked to begin with');
    check(rows().every(r => !r.querySelector('.qedit')),
          'rows must not carry controls when the skin uses one shared set');
    check(window.document.getElementById('qpick').hidden,
          'the shared controls belong to a queued row, not the playing one');

    rows()[0].onclick();
    await new Promise(r => setTimeout(r, 20));
    check(!nowrow.classList.contains('picked'), 'picking a queued row unpicks the playing one');
    check(rows()[0].classList.contains('picked'), 'the picked row must be marked');
    const qp = window.document.getElementById('qpick');
    check(!qp.hidden, 'picking a queued row must reveal the controls');
    check(qp.querySelector('.qedit'), 'the shared set must hold the edit controls');
    // The panel must now describe the PICKED track, not the playing one.
    check(/Queued Reasons/.test(window.document.getElementById('why').textContent),
          'the explanation must follow the pick');
    check(/Next One/.test(window.document.getElementById('whotitle').textContent),
          'the heading must name the track being explained');
    // And they must act on the picked passage, not on whatever was first.
    qp.querySelectorAll('button')[0].onclick(new window.Event('click'));
    check(posted.some(u => u === '/queue/1/remove'),
          `the shared controls must act on the picked passage, posted ${JSON.stringify(posted)}`);

    nowrow.onclick();
    await new Promise(r => setTimeout(r, 20));
    check(nowrow.classList.contains('picked'), 'the playing track must be pickable again');
  }

  // Development mode must be visible, not remembered `[PI-SET-016]`: a
  // notation that names it and a ground that shifts, both driven by the
  // snapshot so a mode left on cannot look like a mode switched off.
  const dev = window.document.getElementById('devmode');
  if (dev) {
    check(dev.hidden, 'diagnostics notation must be hidden when dev_mode is false');
    check(!window.document.body.classList.contains('dev'),
          'the wine ground must be off when dev_mode is false');
    sock.onmessage({ data: JSON.stringify({ ...RICH, dev_mode: true }) });
    check(!dev.hidden, 'diagnostics notation must appear when dev_mode is true');
    check(window.document.body.classList.contains('dev'),
          'the ground must shift when dev_mode is true');
    sock.onmessage({ data: JSON.stringify(RICH) });
    check(dev.hidden, 'and clear again when it goes off');
  }

  const expectedPosts = gear ? (nowrow ? 5 : 4) : 2;
  const ok = errors.length === 0 && posted.length === expectedPosts
             && opts === skins.length && posted[1] === '/volume/-18';
  if (!ok) failures++;
  console.log(
    `${skin.padEnd(11)} ${ok ? 'OK  ' : 'FAIL'}  ` +
    `title=${JSON.stringify((title && title.textContent) || '').slice(0, 30).padEnd(32)} ` +
    `badges=${badges.length} ` +
    `queue=${queue ? queue.children.length : '-'} rows  posted=${JSON.stringify(posted)}  ` +
    `picker=${opts}`);
  for (const e of errors) console.log('    ! ' + e);
}


// ---------------------------------------------------------------------------
// The browse page. Its own check, because it is not a skin: it has no socket,
// it drives three listings and a selection, and it is where the last several
// faults landed -- an empty listing that was really a failed query, a dead end
// on an artist, and an order that came out backwards.
async function runBrowse() {
  const html = fs.readFileSync(path.join(ROOT, 'browse.html'), 'utf8');
  const dom = new JSDOM(html, { runScripts: 'dangerously', url: 'http://localhost/browse' });
  const { window } = dom;
  const errors = [];
  window.console.error = (...a) => errors.push(a.join(' '));

  const create = window.document.createElement.bind(window.document);
  window.document.createElement = tag => {
    const el = create(tag);
    if (tag === 'link' || tag === 'script') setTimeout(() => el.onload && el.onload(), 0);
    return el;
  };

  const ARTISTS = [{ name: 'ABBA', artist: null, passages: 3, plays: 9 },
                   { name: 'Steely Dan', artist: null, passages: 8, plays: 40 }];
  const ALBUMS = [{ name: 'Aja', artist: 'Steely Dan', passages: 7, plays: 30 }];
  const TRACKS = [
    { passage_id: 11, title: 'Black Cow', artist: 'Steely Dan', album: 'Aja', plays: 4, track_no: 1, disc_no: 1 },
    { passage_id: 12, title: 'Aja', artist: 'Steely Dan', album: 'Aja', plays: 2, track_no: 2, disc_no: 1 },
  ];

  const posted = [];
  let fail = null;                       // set to make /browse/* answer 500
  window.fetch = (url, opts) => {
    if (opts && opts.method === 'POST') { posted.push(url); return Promise.resolve({ ok: true }); }
    if (url === '/skins') return Promise.resolve({ json: () => Promise.resolve([]) });
    const m = /^\/browse\/(\w+)/.exec(url);
    if (m) {
      if (fail) return Promise.resolve({ ok: false, status: fail });
      const rows = { artists: ARTISTS, albums: ALBUMS, tracks: TRACKS }[m[1]] || [];
      return Promise.resolve({ ok: true, json: () => Promise.resolve(rows) });
    }
    return Promise.reject(new Error('unexpected fetch ' + url));
  };

  const runScript = src => {
    const el = create('script');
    el.textContent = src;
    window.document.body.appendChild(el);
  };
  runScript(fs.readFileSync(path.join(ROOT, 'core.js'), 'utf8'));
  runScript(fs.readFileSync(path.join(ROOT, 'browse.js'), 'utf8'));

  const settle = () => new Promise(r => setTimeout(r, 30));
  await settle(); await settle();

  const $ = id => window.document.getElementById(id);
  const rows = () => [...window.document.querySelectorAll('#rows li')]
    .filter(li => !li.classList.contains('letter'));
  const check = (cond, msg) => { if (!cond) errors.push(msg); };

  // Artists list, with the alphabet built from what is actually there.
  check(rows().length === 2, `artists: ${rows().length} rows, want 2`);
  const az = [...window.document.querySelectorAll('#az button')];
  check(az.length === 27, `alphabet: ${az.length} letters, want 27`);
  check(az.some(b => b.textContent === 'A' && !b.disabled), 'A should be live');
  check(az.some(b => b.textContent === 'Q' && b.disabled), 'Q should be disabled');
  check($('verbs').hidden, 'the verbs belong to the track listing only');

  // An artist narrows to albums; an album narrows to its tracks.
  rows()[1].onclick();
  await settle();
  check($('crumbs').textContent.includes('Steely Dan'), 'crumb trail should name the artist');
  rows()[0].onclick();
  await settle();
  check(!$('verbs').hidden, 'the verbs appear on tracks');
  check(rows().length === 2, `album tracks: ${rows().length}, want 2`);
  // Album order, so the number leads and the alphabet is gone.
  check(rows()[0].textContent.includes('1. Black Cow'), 'track number should lead in album order');
  check(window.document.querySelectorAll('#az button').length === 0,
        'no alphabet over a running order');

  // Nothing selected: the verbs must refuse rather than act.
  check($('v-now').disabled && $('v-next').disabled && $('v-last').disabled,
        'verbs must start disabled');
  $('v-next').onclick();
  check(posted.length === 0, 'a disabled verb must not post');

  // Select both, in listing order, and queue them as one request.
  for (const box of window.document.querySelectorAll('.pick')) { box.checked = true; }
  window.document.querySelector('.pick').onchange();
  check(!$('v-next').disabled, 'a selection must arm the verbs');
  $('v-next').onclick();
  await settle();
  check(posted.length === 1, `one request, got ${posted.length}`);
  check(posted[0] === '/queue/11,12/next',
        `must send the list in listing order, got ${posted[0]}`);

  // A failed query must say so, not render as an empty library.
  fail = 500;
  $('q').value = 'zzz';
  $('q').oninput();
  await new Promise(r => setTimeout(r, 260));
  check(/could not read/i.test($('note').textContent),
        `a failed query must be reported, got "${$('note').textContent}"`);

  console.log(`${'browse'.padEnd(11)} ${errors.length ? 'FAIL' : 'OK  '}  ` +
              `${rows().length} rows rendered, posted=${JSON.stringify(posted)}`);
  for (const e of errors) console.log('    ! ' + e);
  if (errors.length) failures++;
}

// ---------------------------------------------------------------------------
// The review page. It records judgements that later change what a passage IS,
// so the things worth pinning are the refusals: only contradictions listed, no
// reassignment without a chosen candidate, and a failed write that does not
// leave a card looking decided.
async function runReview() {
  const html = fs.readFileSync(path.join(ROOT, 'review.html'), 'utf8');
  const dom = new JSDOM(html, { runScripts: 'dangerously', url: 'http://localhost/review' });
  const { window } = dom;
  const errors = [];
  window.console.error = (...a) => errors.push(a.join(' '));
  const create = window.document.createElement.bind(window.document);
  window.document.createElement = tag => {
    const el = create(tag);
    if (tag === 'link' || tag === 'script') setTimeout(() => el.onload && el.onload(), 0);
    return el;
  };

  const QUEUE = {
    progress: { ran: true, checked: 8078, confirmed: 6500, contradicted: 2, decided: 0 },
    items: [
      { passage_id: 21, stored_mbid: 'rec-wrong', title: 'Wrong Song',
        artist: 'Some Band', album: 'Some Record', score: 0.97,
        severity: 'wrong-song', rank: 0,
        suggested: [{ mbid: 'rec-real', title: 'Right Song', artist: 'A Band', score: 0.97 }] },
      { passage_id: 22, stored_mbid: 'rec-other', title: 'Another', artist: null,
        album: null, score: 0.93, severity: 'wrong-song', rank: 0, suggested: [] },
      // The bulk of a real library, and off by default: if these render
      // without being asked for, the serious cases are buried again.
      { passage_id: 23, stored_mbid: 'rec-press', title: 'Why Worry',
        artist: 'Dire Straits', album: 'Brothers in Arms', score: 0.99,
        severity: 'different-id', rank: 3,
        suggested: [{ mbid: 'rec-51', title: 'Why Worry (5.1 mix)',
                      artist: 'Dire Straits', score: 0.99 }] },
      { passage_id: 24, stored_mbid: 'rec-unk', title: 'Obscure', artist: null,
        album: null, score: null, severity: 'unverified', rank: 5, suggested: [] },
      // Already judged in an earlier session, and already written into the
      // library -- the case undo must refuse rather than silently drop.
      { passage_id: 25, stored_mbid: 'rec-done', title: 'Settled', artist: 'Someone',
        album: null, score: 0.95, severity: 'wrong-song', rank: 1, suggested: [],
        decision: 'reassigned', chosen_mbid: 'rec-new', applied: true },
    ],
  };
  const RELEASES = [
    { mbid: 'rel-1', title: 'The Album', date: '1985-01-01', status: 'Official',
      track_count: 12, chosen: false },
    { mbid: 'rel-2', title: 'A Later Compilation', date: '2004-01-01',
      status: 'Official', track_count: 20, chosen: false },
  ];
  const posted = [];
  let writeFails = false;
  window.fetch = (url, opts) => {
    if (opts && opts.method === 'POST') {
      posted.push(url);
      if (/\/25\/reopen$/.test(url)) {
        // What the server says about withdrawing an applied decision.
        return Promise.resolve({ ok: false, status: 409,
          text: () => Promise.resolve('already applied to the library; use --revert') });
      }
      return Promise.resolve({ ok: !writeFails, status: writeFails ? 500 : 204,
                               text: () => Promise.resolve('') });
    }
    if (url === '/skins') return Promise.resolve({ json: () => Promise.resolve([]) });
    if (url === '/review/queue')
      return Promise.resolve({ ok: true, json: () => Promise.resolve(QUEUE) });
    if (url.startsWith('/review/releases/'))
      return Promise.resolve({ ok: true, json: () => Promise.resolve(RELEASES) });
    return Promise.resolve({ ok: false, status: 404 });
  };
  // jsdom does not fetch `<script src>`; the page's scripts are injected the
  // same way the other checks do it, as classic scripts so `Vaino` is visible.
  const runScript = src => {
    const el = create('script');
    el.textContent = src;
    window.document.body.appendChild(el);
  };
  runScript(fs.readFileSync(path.join(ROOT, 'core.js'), 'utf8'));
  runScript(fs.readFileSync(path.join(ROOT, 'review.js'), 'utf8'));

  const $ = id => window.document.getElementById(id);
  const settle = () => new Promise(r => setTimeout(r, 30));
  await settle(); await settle();

  const check = (cond, msg) => { if (!cond) errors.push(msg); };
  const cards = () => [...window.document.querySelectorAll('.card')];
  const chip = name => [...window.document.querySelectorAll('.chip')]
                         .find(b => b.className.includes(name));

  // Severity triage is the point of the page: on this library the serious
  // cases are 41 against 526, so anything that renders the bulk by default
  // buries them.
  check(cards().length === 2, `${cards().length} cards shown, want the 2 serious ones`);
  check(!cards().some(c => c.dataset.passage === '23'),
        'a different-pressing case must not show until asked for');
  check(/8,?078/.test($('tally').textContent), 'the tally should report what was checked');

  // Every grade gets a chip carrying its own count, so the size of each kind
  // of problem is visible before choosing what to work through.
  check(chip('different-id'), 'no chip for different-id');
  check(/1\b/.test(chip('different-id').textContent),
        `the chip should carry its count, got "${chip('different-id').textContent}"`);
  check(chip('different-id').getAttribute('aria-pressed') === 'false',
        'different-id must start off');
  check(chip('wrong-song').getAttribute('aria-pressed') === 'true',
        'wrong-song must start on');

  chip('different-id').onclick();
  check(cards().length === 3, `after enabling, ${cards().length} cards, want 3`);
  chip('wrong-song').onclick();
  check(cards().length === 1, `after disabling wrong-song, ${cards().length}, want 1`);
  chip('wrong-song').onclick();          // back to the starting selection
  chip('different-id').onclick();
  check(cards().length === 2, 'toggling back must restore the original list');

  const first = cards()[0];
  const btn = label => [...first.querySelectorAll('button')]
                         .find(b => b.textContent === label);

  // A reassignment with nothing chosen must be impossible to send.
  check(btn('Use the match').disabled, '"Use the match" must start disabled');
  check(/choose one/i.test(btn('Use the match').title || ''),
        'a disabled button must say what would enable it');
  btn('Use the match').onclick();
  check(posted.length === 0, 'an unarmed reassignment must not post');

  // Passage 22 has no candidates at all -- 23 of the 44 no-mbid cards on the
  // real library are like this, and no-mbid leads the queue, so it is the
  // first thing a reviewer meets. A control that can never work must not be
  // offered: greyed says "not yet", and the truth is "not ever, here".
  const noCand = cards().find(c => c.dataset.passage === '22');
  const noCandUse = [...noCand.querySelectorAll('button')]
                      .find(b => b.textContent === 'Use the match');
  check(noCandUse && noCandUse.hidden,
        '"Use the match" must be hidden when there is nothing to match to');
  check(/nothing to compare|nothing here to reassign/i.test(noCand.textContent),
        'a card with no candidates must say why, not just show an empty list');

  // Auditioning goes through the ordinary queue verb rather than a new route.
  btn('Play now').onclick();
  check(posted[0] === '/queue/21/now', `audition should queue the passage, got ${posted[0]}`);

  // A real click, not a synthesised change event: dispatching `change` by
  // hand proves the handler works, which is not the same as proving a person
  // clicking the control reaches it.
  const radio = first.querySelector('input[type=radio]');
  radio.click();
  check(!btn('Use the match').disabled, 'choosing a candidate must arm the button');

  // Choosing a candidate offers the albums it appears on. Without an answer
  // the album is picked by release date, which is a guess.
  await settle();
  const sel = first.querySelector('.albums select');
  check(sel, 'choosing a candidate should offer which album to call it');
  if (sel) {
    check(sel.options.length === RELEASES.length + 1,
          `${sel && sel.options.length} album options, want ${RELEASES.length + 1} with "leave as is"`);
    check(sel.options[0].value === '', 'leaving the album alone must be an option');
    sel.value = 'rel-2';
  }

  btn('Use the match').onclick();
  await settle();
  check(posted[1] === '/review/21/reassigned?mbid=rec-real&release=rel-2',
        `reassignment must carry recording and album, got ${posted[1]}`);
  check(first.classList.contains('done'), 'a settled card should read as settled');

  // Undo. It appears only once a decision exists, and puts the card back.
  const undo = () => btn('Undo');
  check(undo() && !undo().hidden, 'a settled card must offer an undo');
  undo().onclick();
  await settle();
  check(posted[2] === '/review/21/reopen', `undo should reopen, got ${posted[2]}`);
  const reopened = cards().find(c => c.dataset.passage === '21');
  check(reopened && !reopened.classList.contains('done'),
        'after undo the card must be answerable again');

  // Decided cards from earlier sessions are off by default and reachable.
  check(!cards().some(c => c.dataset.passage === '25'),
        'a previously-decided card must not clutter the working queue');
  const decidedChip = [...window.document.querySelectorAll('.chip')]
                        .find(b => b.className.includes('decided'));
  check(decidedChip, 'no chip for decided');
  decidedChip.onclick();
  await settle();
  const settled = cards().find(c => c.dataset.passage === '25');
  check(settled, 'the decided chip must reveal earlier judgements');
  if (settled) {
    check(/applied/.test(settled.textContent),
          'a decision already written to the library must say so');
    // Undo must refuse it, and the reason has to be readable.
    const u = [...settled.querySelectorAll('button')].find(b => b.textContent === 'Undo');
    u.onclick();
    await settle();
    check(/--revert/.test(settled.textContent),
          `a refused undo must explain itself, got "${settled.textContent.slice(-80)}"`);
  }

  // A card with no candidates can still be kept -- that is a real answer.
  const second = cards()[1];
  const keep = [...second.querySelectorAll('button')].find(b => b.textContent === 'Keep ours');
  writeFails = true;
  keep.onclick();
  await settle();
  check(!second.classList.contains('done'),
        'a FAILED write must not leave the card looking decided');
  check(!keep.disabled, 'a failed write must re-arm the controls');

  console.log(`${'review'.padEnd(11)} ${errors.length ? 'FAIL' : 'OK  '}  ` +
              `${cards().length} cards, posted=${JSON.stringify(posted)}`);
  for (const e of errors) console.log('    ! ' + e);
  if (errors.length) failures++;
}

(async () => {
  for (const s of skins) await run(s);
  await runBrowse();
  await runReview();
  console.log(failures ? `\n${failures} skin(s) failed` : '\nall skins rendered without error');
  process.exit(failures ? 1 : 0);
})();
