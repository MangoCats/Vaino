// Waveform boundary editing [REQ-LIB-175], [SPEC021].
//
// Fetch the passage's current boundaries and its raw audio, decode it
// client-side, and draw a peak waveform with four draggable markers and a
// gain field over it. A drag previews on release, not on every pixel of
// motion `[SPEC021 §4]` -- decoding audio on every frame of a drag is where
// a "real-time" editor stops feeling real-time. "Save edit" posts the draft
// to `boundary_reviews`; nothing here ever touches `passages` itself
// `[SPEC-SUI-140]`.
//
// A zoomable/pannable viewport, precise numeric fields, undo, and pausing
// the main transport were added `[SPEC-SUI-217..220]` after using this
// editor against a real multi-minute file: a fixed-width canvas mapping the
// whole file to one screen made a lead-in a few pixels wide, the only way to
// grab a marker was a blind 10px guess on a bare line, there was no way back
// from a mistake, and the server's own playback kept running underneath the
// browser's preview with no way to silence it from here.
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

  // The viewport: what span of the file the canvas currently shows. Distinct
  // from the passage's own start/end -- "what you can see" and "what the
  // passage is" are different questions, and conflating them is why the old
  // single fixed-width canvas made a 40ms lead-in impossible to see, let
  // alone drag, on a four-minute file `[SPEC-SUI-218]`.
  let view = { fromMs: 0, toMs: 0 };
  const MIN_SPAN_MS = 200;

  function clampView(fromMs, toMs) {
    const total = totalMs();
    const cap = total || MIN_SPAN_MS;
    let span = clamp(toMs - fromMs, Math.min(MIN_SPAN_MS, cap), cap);
    const from = clamp(fromMs, 0, Math.max(0, total - span));
    return { fromMs: from, toMs: from + span };
  }
  function setView(fromMs, toMs) {
    view = clampView(fromMs, toMs);
    $('viewrange').textContent =
      `showing ${fmt(view.fromMs)} – ${fmt(view.toMs)} of ${fmt(totalMs())}`;
    draw();
  }
  function zoomStep(factor) {
    const mid = (view.fromMs + view.toMs) / 2;
    const span = (view.toMs - view.fromMs) * factor;
    setView(mid - span / 2, mid + span / 2);
  }

  const msOf = x => {
    const w = canvas.getBoundingClientRect().width || canvas.width;
    return view.fromMs + (x / w) * (view.toMs - view.fromMs);
  };

  const dirty = () =>
    !base ||
    draft.start_ms !== base.start_ms ||
    draft.end_ms !== base.end_ms ||
    draft.lead_in_ms !== base.lead_in_ms ||
    draft.lead_out_ms !== base.lead_out_ms ||
    Math.abs(draft.gain_db - base.gain_db) > 1e-9;

  const refreshCommitState = () => { $('commit').disabled = !dirty(); };

  // Undo: one stack of prior drafts, pushed once per *completed* edit -- a
  // drag's own motion, or a field's own keystrokes, never touch it, matching
  // the "preview on release, not per-pixel" rule this file already followed
  // for playback `[SPEC-SUI-220]`.
  let undoStack = [];
  const UNDO_CAP = 50;
  function pushUndo() {
    if (!draft) return;
    undoStack.push({ ...draft });
    if (undoStack.length > UNDO_CAP) undoStack.shift();
    $('undo').disabled = false;
  }
  function undo() {
    if (!undoStack.length) return;
    stopPreview();
    draft = undoStack.pop();
    $('undo').disabled = undoStack.length === 0;
    updateFacts();
    updatePreciseFields();
    $('gain').value = draft.gain_db.toFixed(1);
    refreshCommitState();
    draw();
  }
  $('undo').addEventListener('click', undo);

  async function load() {
    if (!passageId) {
      note('No passage id in the URL.', 'bad');
      return;
    }

    // Two audio engines can otherwise play the same passage at once: the
    // server's own transport, which runs regardless of any browser tab, and
    // this page's own preview. Silencing the server side once, on entry, is
    // simpler and more reliable than trying to keep both in sync
    // `[SPEC-SUI-217]`. Best-effort -- a failed pause is not worth blocking
    // the editor over.
    fetch('/command/pause', { method: 'POST' }).catch(() => {});
    $('pausenote').textContent =
      'The main player was paused so only this preview plays. Its own Play ' +
      'button on the player page resumes it when you are done here.';

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
    updatePreciseFields();
    for (const id of ['startms', 'endms', 'leadinms', 'leadoutms']) $(id).disabled = false;

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
    for (const id of ['zoomout', 'zoomin', 'zoompassage', 'zoomfile']) $(id).disabled = false;
    setView(0, totalMs());
    note(info.edited ? 'Showing a saved-but-not-yet-applied edit.' : '');
  }

  function updateFacts() {
    $('facts').innerHTML =
      `start <b>${fmt(draft.start_ms)}</b> · end <b>${fmt(draft.end_ms)}</b> · ` +
      `lead-in <b>${draft.lead_in_ms} ms</b> · lead-out <b>${draft.lead_out_ms} ms</b> · ` +
      `gain <b>${draft.gain_db.toFixed(2)} dB</b> · file <b>${fmt(fileMs || null)}</b>`;
  }

  function updatePreciseFields() {
    $('startms').value = draft.start_ms;
    $('endms').value = draft.end_ms;
    $('leadinms').value = draft.lead_in_ms;
    $('leadoutms').value = draft.lead_out_ms;
  }

  // One min/max pair per horizontal pixel `[SPEC021 §4]`, over whatever span
  // the viewport currently shows rather than always the whole buffer -- the
  // same cost per redraw either way, since it is still one pass over `w`
  // columns; only which samples fall in each column changes.
  function draw() {
    // Nothing to draw before the audio decodes, and nowhere to draw it in an
    // environment with no real canvas (this file's own jsdom check among
    // them) -- both are "there is nothing here yet," not an error.
    if (!buffer) return;
    const rect = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.max(1, Math.round((rect.width || 1200) * dpr));
    canvas.height = Math.max(1, Math.round(220 * dpr));
    const g = canvas.getContext('2d');
    if (!g) return;
    const w = canvas.width, h = canvas.height;
    g.clearRect(0, 0, w, h);

    const data = buffer.getChannelData(0);
    const sr = buffer.sampleRate;
    const span = Math.max(1, view.toMs - view.fromMs);
    const mid = h / 2;
    g.strokeStyle = '#9ad';
    g.lineWidth = Math.max(1, dpr);
    g.beginPath();
    for (let x = 0; x < w; x++) {
      const msLo = view.fromMs + (x / w) * span;
      const msHi = view.fromMs + ((x + 1) / w) * span;
      const lo = Math.max(0, Math.floor((msLo / 1000) * sr));
      const hi = Math.min(data.length, Math.max(lo + 1, Math.ceil((msHi / 1000) * sr)));
      let min = 1, max = -1;
      for (let i = lo; i < hi; i++) {
        const v = data[i];
        if (v < min) min = v;
        if (v > max) max = v;
      }
      if (min > max) { min = 0; max = 0; }
      g.moveTo(x, mid + min * mid);
      g.lineTo(x, mid + max * mid);
    }
    g.stroke();

    const px = ms => ((ms - view.fromMs) / span) * w;
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

    // A knob near the top of each line -- an actual grabbable shape, not
    // just a place on a line that happens to respond to a drag
    // `[SPEC-SUI-219]`. `handleAt` only recognises a pointer this close to
    // the top of the canvas as "on a handle" at all, so the rest of the
    // canvas is unambiguously pan/click territory.
    const knob = (ms, color) => {
      const x = px(ms);
      g.fillStyle = color;
      g.beginPath();
      g.arc(x, HANDLE_BAND_PX * 0.55 * dpr, 5 * dpr, 0, Math.PI * 2);
      g.fill();
    };
    knob(draft.start_ms, '#6cf');
    knob(draft.end_ms, '#6cf');
    knob(draft.start_ms + draft.lead_in_ms, '#fb6');
    knob(draft.end_ms - draft.lead_out_ms, '#fb6');

    if (source) {
      const elapsed = (audioCtx.currentTime - playedAt) * 1000;
      line(draft.start_ms + playedFromMs + elapsed, '#fff', 1.5);
    }
  }

  // Which marker a pointer is close enough to grab, and only near the top of
  // the canvas -- the knob rail. Below that band every pointer gesture is
  // pan-or-click, never a marker drag, so there is no blind guessing about
  // which one a click near the middle of the canvas meant `[SPEC-SUI-219]`.
  const HANDLE_BAND_PX = 28;
  function handleAt(x, y) {
    if (y > HANDLE_BAND_PX) return null;
    const w = canvas.getBoundingClientRect().width || canvas.width;
    const span = Math.max(1, view.toMs - view.fromMs);
    const tolerancePx = 12;
    const candidates = [
      ['leadIn', draft.start_ms + draft.lead_in_ms],
      ['leadOut', draft.end_ms - draft.lead_out_ms],
      ['start', draft.start_ms],
      ['end', draft.end_ms],
    ];
    let best = null, bestDist = tolerancePx;
    for (const [name, ms] of candidates) {
      const d = Math.abs(((ms - view.fromMs) / span) * w - x);
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

  // Pan vs. click-to-jump, told apart by how far the pointer actually moved
  // `[SPEC-SUI-218]`: a gesture that never crosses the threshold is a click
  // (jump playback there); one that does is a pan (move the viewport, touch
  // nothing else). A gesture that starts on a knob is neither -- it drags
  // that marker, exactly as before.
  const PAN_THRESHOLD_PX = 4;
  let panStart = null; // { x, fromMs, toMs }
  let didPan = false;

  canvas.addEventListener('pointerdown', e => {
    if (!draft || !buffer) return;
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    dragging = handleAt(x, y);
    canvas.setPointerCapture(e.pointerId);
    if (dragging) {
      pushUndo();
      return;
    }
    panStart = { x, fromMs: view.fromMs, toMs: view.toMs };
    didPan = false;
  });
  canvas.addEventListener('pointermove', e => {
    const rect = canvas.getBoundingClientRect();
    const x = clamp(e.clientX - rect.left, 0, rect.width);
    const y = e.clientY - rect.top;
    if (dragging) {
      applyDrag(dragging, msOf(x));
      updateFacts();
      updatePreciseFields();
      draw();
      return;
    }
    if (panStart) {
      const dx = x - panStart.x;
      if (didPan || Math.abs(dx) >= PAN_THRESHOLD_PX) {
        didPan = true;
        canvas.classList.add('panning');
        const span = panStart.toMs - panStart.fromMs;
        const w = rect.width || canvas.width;
        const deltaMs = (dx / w) * span;
        setView(panStart.fromMs - deltaMs, panStart.toMs - deltaMs);
      }
      return;
    }
    canvas.classList.toggle('onhandle', Boolean(handleAt(x, y)));
  });
  const releaseDrag = e => {
    if (dragging) {
      dragging = null;
      refreshCommitState();
      playFrom(0); // preview from the new start, matching what "release" means in SPEC021 §4
      return;
    }
    if (panStart) {
      if (!didPan && e) {
        const rect = canvas.getBoundingClientRect();
        const x = clamp(e.clientX - rect.left, 0, rect.width);
        playFrom(clamp(msOf(x) - draft.start_ms, 0, draft.end_ms - draft.start_ms));
      }
      panStart = null;
      didPan = false;
      canvas.classList.remove('panning');
    }
  };
  canvas.addEventListener('pointerup', releaseDrag);
  canvas.addEventListener('pointercancel', () => {
    dragging = null;
    panStart = null;
    didPan = false;
    canvas.classList.remove('panning');
  });

  // Wheel-to-zoom, centred on the cursor rather than the view's own middle --
  // the point someone is pointing at is the point that should stay put
  // `[SPEC-SUI-218]`.
  canvas.addEventListener('wheel', e => {
    if (!buffer) return;
    e.preventDefault();
    const rect = canvas.getBoundingClientRect();
    const x = clamp(e.clientX - rect.left, 0, rect.width);
    const anchorMs = msOf(x);
    const factor = e.deltaY > 0 ? 1.25 : 0.8;
    const span = (view.toMs - view.fromMs) * factor;
    const ratio = (rect.width || canvas.width) ? x / (rect.width || canvas.width) : 0.5;
    setView(anchorMs - span * ratio, anchorMs + span * (1 - ratio));
  }, { passive: false });

  $('zoomin').addEventListener('click', () => zoomStep(0.5));
  $('zoomout').addEventListener('click', () => zoomStep(2));
  $('zoompassage').addEventListener('click', () => setView(draft.start_ms, draft.end_ms));
  $('zoomfile').addEventListener('click', () => setView(0, totalMs()));

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
      // Detach `onended` before stopping it: the event still fires
      // asynchronously after `.stop()`, and by the time it does, `source`
      // may already have moved on to a newer node started in the meantime.
      // A callback still watching the old node would then see `source ===`
      // that stale node and wrongly null out the *current* one -- this is
      // the actual cause a click could seem to "start another stream" that
      // never got silenced: the reference to it was lost, so a later
      // `stopPreview()` had nothing to call `.stop()` on, even though the
      // node itself was still actually playing.
      source.onended = null;
      try { source.stop(); } catch (e) { /* already stopped */ }
      source.disconnect();
      source = null;
    }
    $('play').textContent = 'Play';
  }

  function playFrom(offsetMs) {
    stopPreview();
    const preview = renderPreview();
    const node = audioCtx.createBufferSource();
    node.buffer = preview;
    node.connect(audioCtx.destination);
    const offSec = clamp(offsetMs, 0, preview.duration * 1000) / 1000;
    playedAt = audioCtx.currentTime;
    playedFromMs = offSec * 1000;
    // Guarded by identity, not just detached above: a node that runs to its
    // own natural end (never stopped by hand) must still clear `source`, but
    // only if nothing newer has already taken its place.
    node.onended = () => {
      if (source === node) { source = null; $('play').textContent = 'Play'; }
    };
    node.start(0, offSec);
    source = node;
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
      pushUndo();
      draft.gain_db = v;
      updateFacts();
      refreshCommitState();
    }
  });

  // The precise fields: the same five values dragging a knob edits, typed
  // instead of dragged -- most of what "how do I place these" was really
  // asking for `[SPEC-SUI-219]`. Same clamps `applyDrag` already enforces,
  // so a typed value cannot reach a state a drag could not also reach.
  $('startms').addEventListener('change', () => {
    const v = Number($('startms').value);
    if (!Number.isFinite(v)) return updatePreciseFields();
    pushUndo();
    draft.start_ms = clamp(Math.round(v), 0, draft.end_ms - 1);
    updateFacts(); updatePreciseFields(); draw(); refreshCommitState();
  });
  $('endms').addEventListener('change', () => {
    const v = Number($('endms').value);
    if (!Number.isFinite(v)) return updatePreciseFields();
    pushUndo();
    draft.end_ms = clamp(Math.round(v), draft.start_ms + 1, totalMs());
    updateFacts(); updatePreciseFields(); draw(); refreshCommitState();
  });
  $('leadinms').addEventListener('change', () => {
    const v = Number($('leadinms').value);
    if (!Number.isFinite(v)) return updatePreciseFields();
    pushUndo();
    draft.lead_in_ms = Math.round(clamp(v, 0, draft.end_ms - draft.start_ms));
    updateFacts(); updatePreciseFields(); draw(); refreshCommitState();
  });
  $('leadoutms').addEventListener('change', () => {
    const v = Number($('leadoutms').value);
    if (!Number.isFinite(v)) return updatePreciseFields();
    pushUndo();
    draft.lead_out_ms = Math.round(clamp(v, 0, draft.end_ms - draft.start_ms));
    updateFacts(); updatePreciseFields(); draw(); refreshCommitState();
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
    // What has and has not happened: this writes `boundary_reviews` on THIS
    // machine's own database only `[SPEC021 §2]`. It is not yet folded into
    // `passages` here (that is `apply_boundary_reviews.py`, a deliberate
    // separate step), and it has not gone anywhere else at all -- carrying it
    // to another library is Sampo's own sync `[SPEC-DF-107..118]`, not
    // something this page can reach or should try to trigger.
    note('Saved to this machine’s own database. Not yet applied to the ' +
         'library here, and not yet sent to any other library -- both are ' +
         'separate, deliberate steps.', 'good');
  });

  load().catch(() => note('Something went wrong loading this passage.', 'bad'));
})();
