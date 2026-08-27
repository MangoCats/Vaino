// Waveform boundary editing [REQ-LIB-175], [SPEC021].
//
// Stage 6 only [IMPL004]: fetch the passage's current boundaries and its raw
// audio, decode it client-side, and draw a peak waveform with the boundaries
// marked -- undraggable. Dragging the markers, the real-time preview
// transport and the commit to `boundary_reviews` are Stage 7. Nothing here
// writes anywhere; it costs nothing to ship ahead of that.
(() => {
  const $ = id => document.getElementById(id);
  const note = (text, bad = false) => {
    const n = $('note');
    n.textContent = text;
    n.classList.toggle('bad', bad);
  };

  Vaino.startBare();

  // `/edit/:passage_id` is a static shell -- the id lives in the URL path,
  // not in the response, so the page reads it back out the same way the
  // server did to route here.
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

  async function load() {
    if (!passageId) {
      note('No passage id in the URL.', true);
      return;
    }

    const infoResp = await fetch(`/edit/${passageId}/info`).catch(() => null);
    if (!infoResp || !infoResp.ok) {
      note(`Passage ${passageId} does not exist, or its info could not be read.`, true);
      return;
    }
    const info = await infoResp.json();

    $('facts').innerHTML =
      `start <b>${fmt(info.start_ms)}</b> · end <b>${fmt(info.end_ms)}</b> · ` +
      `lead-in <b>${info.lead_in_ms} ms</b> · lead-out <b>${info.lead_out_ms} ms</b> · ` +
      `gain <b>${info.gain_db.toFixed(2)} dB</b> · file <b>${fmt(info.file_ms || null)}</b>`;

    note('Loading audio…');
    const audioResp = await fetch(`/edit/${passageId}/audio`).catch(() => null);
    if (!audioResp || !audioResp.ok) {
      note('Could not read the audio for this passage.', true);
      return;
    }
    const bytes = await audioResp.arrayBuffer();

    const Ctx = window.AudioContext || window.webkitAudioContext;
    const ctx = new Ctx();
    let buffer;
    try {
      // Safari and Chrome both want a copy of the ArrayBuffer's callback form
      // supported; the promise form is used here and is broadly supported by
      // the desktop browsers this feature targets `[SPEC-SUI-135]`.
      buffer = await ctx.decodeAudioData(bytes);
    } catch (e) {
      note('The audio could not be decoded by this browser.', true);
      return;
    }

    draw(buffer, info);
    note('');
  }

  // One min/max pair per horizontal pixel -- the standard peak-rendering
  // approach `[SPEC021 §4]`: cheap even for a multi-minute capture because it
  // runs once per load, not once per frame.
  function draw(buffer, info) {
    const canvas = $('wave');
    // Match the backing store to the element's on-screen size so the waveform
    // is not stretched or blurred on a resized window.
    const rect = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.max(1, Math.round(rect.width * dpr));
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

    // Boundaries, against the decoded duration -- sample-accurate, and not
    // dependent on whether the library's own `file_ms` guess agrees with it.
    const totalMs = buffer.duration * 1000;
    const xOf = ms => (ms / totalMs) * w;

    const shade = (fromMs, toMs) => {
      if (toMs <= fromMs) return;
      g.fillStyle = 'rgba(255,187,102,.18)';
      g.fillRect(xOf(fromMs), 0, xOf(toMs) - xOf(fromMs), h);
    };
    shade(info.start_ms, info.start_ms + info.lead_in_ms);
    shade(info.end_ms - info.lead_out_ms, info.end_ms);

    const line = (ms, color) => {
      const x = xOf(ms);
      g.strokeStyle = color;
      g.lineWidth = Math.max(2, dpr);
      g.beginPath();
      g.moveTo(x, 0);
      g.lineTo(x, h);
      g.stroke();
    };
    line(info.start_ms, '#6cf');
    line(info.end_ms, '#6cf');
  }

  load().catch(() => note('Something went wrong loading this passage.', true));
})();
