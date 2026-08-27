// Waveform boundary editing [REQ-LIB-175], [SPEC021].
//
// Fetch the passage's current boundaries and its raw audio, decode it
// client-side, and draw a peak waveform with four draggable markers and a
// gain field over it. A drag previews on release, not on every pixel of
// motion `[SPEC021 §4]` -- decoding audio on every frame of a drag is where
// a "real-time" editor stops feeling real-time. "Save edit" posts the draft
// to `boundary_reviews`; nothing here ever touches `passages` itself
// `[SPEC-SUI-140]`.
(() => {
  const $ = id => document.getElementById(id);
  const note = (text, cls = '') => {
    const n = $('note');
    n.textContent = text;
    n.className = 'note' + (cls ? ' ' + cls : '');
  };

  Vaino.startBare();

  const passageId = (() => {
    const m = location.pathname.match(/\/edit\/(\d+)/);
    return m ? Number(m[1]) : null;
  })();

  const fmt = ms => {
    if (ms == null) return '—';
    const s = ms / 1000;
    const m = Math.floor(s / 60);
    const r = (s - m * 60).toFixed(3);
    return `${m}:${r.padStart(6, '0')}`;
  };
  const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));

  let base = null;    // the last-loaded-or-saved values, for the dirty check
  let draft = null;   // the values being edited -- same five fields
  let buffer = null;  // the decoded AudioBuffer
  let audioCtx = null;
  let source = null;  // the currently-playing AudioBufferSourceNode, or null
  let playedAt = 0;   // audioCtx.currentTime when the current playback began
  let playedFromMs = 0; // draft-relative ms the current playback began at
  let dragging = null; // 'start' | 'end' | 'leadIn' | 'leadOut' | null
  let fileMs = 0;      // the file's own duration, for the facts line only

  const canvas = $('wave');

  const totalMs = () => (buffer ? buffer.duration * 1000 : 0);
  const msOf = x => {
    const w = canvas.getBoundingClientRect().width || canvas.width;
    return (x / w) * totalMs();
  };

  const dirty = () =>
    !base ||
    draft.start_ms !== base.start_ms ||
    draft.end_ms !== base.end_ms ||
    draft.lead_in_ms !== base.lead_in_ms ||
    draft.lead_out_ms !== base.lead_out_ms ||
    Math.abs(draft.gain_db - base.gain_db) > 1e-9;

  const refreshCommitState = () => { $('commit').disabled = !dirty(); };

  async function load() {
    if (!passageId) {
      note('No passage id in the URL.', 'bad');
      return;
    }

    const infoResp = await fetch(`/edit/${passageId}/info`).catch(() => null);
    if (!infoResp || !infoResp.ok) {
      note(`Passage ${passageId} does not exist, or its info could not be read.`, 'bad');
      return;
    }
    const info = await infoResp.json();
    base = {
      start_ms: info.start_ms, end_ms: info.end_ms,
      lead_in_ms: info.lead_in_ms, lead_out_ms: info.lead_out_ms,
      gain_db: info.gain_db,
    };
    draft = { ...base };
    fileMs = info.file_ms;
    $('gain').value = draft.gain_db.toFixed(1);
    $('gain').disabled = false;
    updateFacts();

    note('Loading audio…');
    const audioResp = await fetch(`/edit/${passageId}/audio`).catch(() => null);
    if (!audioResp || !audioResp.ok) {
      note('Could not read the audio for this passage.', 'bad');
      return;
    }
    const bytes = await audioResp.arrayBuffer();

    const Ctx = window.AudioContext || window.webkitAudioContext;
    audioCtx = new Ctx();
    try {
      buffer = await audioCtx.decodeAudioData(bytes);
    } catch (e) {
      note('The audio could not be decoded by this browser.', 'bad');
      return;
    }

    $('play').disabled = false;
    draw();
    note(info.edited ? 'Showing a saved-but-not-yet-applied edit.' : '');
  }

  function updateFacts() {
    $('facts').innerHTML =
      `start <b>${fmt(draft.start_ms)}</b> · end <b>${fmt(draft.end_ms)}</b> · ` +
      `lead-in <b>${draft.lead_in_ms} ms</b> · lead-out <b>${draft.lead_out_ms} ms</b> · ` +
      `gain <b>${draft.gain_db.toFixed(2)} dB</b> · file <b>${fmt(fileMs || null)}</b>`;
  }

  // One min/max pair per horizontal pixel `[SPEC021 §4]` -- cheap even for a
  // multi-minute capture because it runs once per load, not once per frame.
  function draw() {
    const rect = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.max(1, Math.round((rect.width || 1200) * dpr));
    canvas.height = Math.max(1, Math.round(220 * dpr));
    const g = canvas.getContext('2d');
    const w = canvas.width, h = canvas.height;
    g.clearRect(0, 0, w, h);

    const data = buffer.getChannelData(0);
    const mid = h / 2;
    g.strokeStyle = '#9ad';
    g.lineWidth = Math.max(1, dpr);
    g.beginPath();
    for (let x = 0; x < w; x++) {
      const lo = Math.floor((x / w) * data.length);
      const hi = Math.max(lo + 1, Math.floor(((x + 1) / w) * data.length));
      let min = 1, max = -1;
      for (let i = lo; i < hi && i < data.length; i++) {
        const v = data[i];
        if (v < min) min = v;
        if (v > max) max = v;
      }
      if (min > max) { min = 0; max = 0; }
      g.moveTo(x, mid + min * mid);
      g.lineTo(x, mid + max * mid);
    }
    g.stroke();

    const px = ms => (ms / totalMs()) * w;
    const shade = (fromMs, toMs) => {
      if (toMs <= fromMs) return;
      g.fillStyle = 'rgba(255,187,102,.18)';
      g.fillRect(px(fromMs), 0, px(toMs) - px(fromMs), h);
    };
    shade(draft.start_ms, draft.start_ms + draft.lead_in_ms);
    shade(draft.end_ms - draft.lead_out_ms, draft.end_ms);

    const line = (ms, color, width) => {
      const x = px(ms);
      g.strokeStyle = color;
      g.lineWidth = width * dpr;
      g.beginPath();
      g.moveTo(x, 0);
      g.lineTo(x, h);
      g.stroke();
    };
    line(draft.start_ms, '#6cf', 2);
    line(draft.end_ms, '#6cf', 2);
    line(draft.start_ms + draft.lead_in_ms, '#fb6', 1.5);
    line(draft.end_ms - draft.lead_out_ms, '#fb6', 1.5);

    if (source) {
      const elapsed = (audioCtx.currentTime - playedAt) * 1000;
      line(draft.start_ms + playedFromMs + elapsed, '#fff', 1.5);
    }
  }

  // Which marker a pointer at canvas-relative `x` is close enough to grab.
  // Handles are checked lead markers first: at a fully-collapsed lead they
  // sit on top of start/end, and the lead is the one a person is more often
  // trying to adjust once it is already at an extreme.
  function handleAt(x) {
    const w = canvas.getBoundingClientRect().width || canvas.width;
    const tolerancePx = 10;
    const candidates = [
      ['leadIn', draft.start_ms + draft.lead_in_ms],
      ['leadOut', draft.end_ms - draft.lead_out_ms],
      ['start', draft.start_ms],
      ['end', draft.end_ms],
    ];
    let best = null, bestDist = tolerancePx;
    for (const [name, ms] of candidates) {
      const d = Math.abs((ms / totalMs()) * w - x);
      if (d < bestDist) { bestDist = d; best = name; }
    }
    return best;
  }

  function applyDrag(which, ms) {
    const total = totalMs();
    if (which === 'start') draft.start_ms = clamp(ms, 0, draft.end_ms - 1);
    else if (which === 'end') draft.end_ms = clamp(ms, draft.start_ms + 1, total);
    else if (which === 'leadIn')
      draft.lead_in_ms = Math.round(clamp(ms - draft.start_ms, 0, draft.end_ms - draft.start_ms));
    else if (which === 'leadOut')
      draft.lead_out_ms = Math.round(clamp(draft.end_ms - ms, 0, draft.end_ms - draft.start_ms));
  }

  canvas.addEventListener('pointerdown', e => {
    if (!draft || !buffer) return;
    const x = e.clientX - canvas.getBoundingClientRect().left;
    dragging = handleAt(x);
    if (dragging) {
      canvas.setPointerCapture(e.pointerId);
    } else {
      // Not on a handle: seek-and-play from where the click landed.
      playFrom(clamp(msOf(x) - draft.start_ms, 0, draft.end_ms - draft.start_ms));
    }
  });
  canvas.addEventListener('pointermove', e => {
    if (!dragging) return;
    const x = clamp(e.clientX - canvas.getBoundingClientRect().left, 0, canvas.getBoundingClientRect().width);
    applyDrag(dragging, msOf(x));
    updateFacts();
    draw();
  });
  const releaseDrag = () => {
    if (!dragging) return;
    dragging = null;
    refreshCommitState();
    playFrom(0); // preview from the new start, matching what "release" means in SPEC021 §4
  };
  canvas.addEventListener('pointerup', releaseDrag);
  canvas.addEventListener('pointercancel', releaseDrag);

  // The preview player: a fresh `AudioBuffer` sliced at the draft's
  // boundaries with the draft's fades and gain baked in sample-by-sample,
  // using the exact same formula `fade.rs` applies to real playback
  // `[SPEC021 §4]` -- so what changes on a drag is also what is heard.
  function renderPreview() {
    const sr = buffer.sampleRate;
    const startSample = Math.max(0, Math.floor((draft.start_ms / 1000) * sr));
    const endSample = Math.min(buffer.length, Math.ceil((draft.end_ms / 1000) * sr));
    const n = Math.max(1, endSample - startSample);
    const leadIn = Math.round((draft.lead_in_ms / 1000) * sr);
    const leadOut = Math.round((draft.lead_out_ms / 1000) * sr);
    const foStart = n - leadOut;
    const linGain = Math.pow(10, draft.gain_db / 20);
    const out = audioCtx.createBuffer(buffer.numberOfChannels, n, sr);
    for (let c = 0; c < buffer.numberOfChannels; c++) {
      const src = buffer.getChannelData(c);
      const dst = out.getChannelData(c);
      for (let i = 0; i < n; i++) {
        let g = linGain;
        if (leadIn > 0 && i < leadIn) g *= VainoFade.gainIn(i / leadIn);
        if (leadOut > 0 && i >= foStart) g *= VainoFade.gainOut((i - foStart) / leadOut);
        dst[i] = (src[startSample + i] || 0) * g;
      }
    }
    return out;
  }

  function stopPreview() {
    if (source) {
      try { source.stop(); } catch (e) { /* already stopped */ }
      source.disconnect();
      source = null;
    }
    $('play').textContent = 'Play';
  }

  function playFrom(offsetMs) {
    stopPreview();
    const preview = renderPreview();
    source = audioCtx.createBufferSource();
    source.buffer = preview;
    source.connect(audioCtx.destination);
    const offSec = clamp(offsetMs, 0, preview.duration * 1000) / 1000;
    playedAt = audioCtx.currentTime;
    playedFromMs = offSec * 1000;
    source.onended = () => { source = null; $('play').textContent = 'Play'; };
    source.start(0, offSec);
    $('play').textContent = 'Pause';
    requestAnimationFrame(tick);
  }

  function tick() {
    draw();
    if (source) requestAnimationFrame(tick);
  }

  $('play').addEventListener('click', () => {
    if (source) stopPreview();
    else playFrom(0);
  });

  $('gain').addEventListener('change', () => {
    const v = Number($('gain').value);
    if (Number.isFinite(v)) {
      draft.gain_db = v;
      updateFacts();
      refreshCommitState();
    }
  });

  $('commit').addEventListener('click', async () => {
    $('commit').disabled = true;
    const resp = await fetch(`/edit/${passageId}/review`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(draft),
    }).catch(() => null);
    if (!resp || !resp.ok) {
      const text = resp ? await resp.text().catch(() => '') : '';
      note(`Could not save the edit${text ? `: ${text}` : ''}.`, 'bad');
      refreshCommitState();
      return;
    }
    base = { ...draft };
    note('Saved. Not yet applied to the library -- that is a separate step.', 'good');
  });

  load().catch(() => note('Something went wrong loading this passage.', 'bad'));
})();
