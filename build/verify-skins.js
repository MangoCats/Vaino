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
  skip: { fade_ms: 0, lead_ms: 500, fade_max_ms: 10000, lead_min_ms: 100, lead_max_ms: 2000 },
  underrun_samples: 0, why: null,
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
  skip: { fade_ms: 2000, lead_ms: 500, fade_max_ms: 10000, lead_min_ms: 100, lead_max_ms: 2000 },
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
  const ok = errors.length === 0 && posted.length === 2 && opts === skins.length
             && posted[1] === '/volume/-18';
  if (!ok) failures++;
  console.log(
    `${skin.padEnd(11)} ${ok ? 'OK  ' : 'FAIL'}  ` +
    `title=${JSON.stringify((title && title.textContent) || '').slice(0, 30).padEnd(32)} ` +
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

(async () => {
  for (const s of skins) await run(s);
  await runBrowse();
  console.log(failures ? `\n${failures} skin(s) failed` : '\nall skins rendered without error');
  process.exit(failures ? 1 : 0);
})();
