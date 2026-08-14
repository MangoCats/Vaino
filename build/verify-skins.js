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
  const dom = new JSDOM(fs.readFileSync(path.join(ROOT, 'shell.html'), 'utf8'),
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

(async () => {
  for (const s of skins) await run(s);
  console.log(failures ? `\n${failures} skin(s) failed` : '\nall skins rendered without error');
  process.exit(failures ? 1 : 0);
})();
