// Shared bits of the Sampo console [SPEC013].
//
// Small on purpose. The console has no live state to push -- a library does not
// change while you look at it unless a job is running, and jobs are stage 3 --
// so there is no socket here and no snapshot dispatch. When stage 3 adds jobs
// it adds an EventSource, which is the smaller mechanism that fits a
// one-directional progress stream [SPEC-SUI-030].

const S = {
  async get(path) {
    const r = await fetch(path);
    const d = await r.json();
    // A failed query reports rather than rendering as an empty library. That
    // exact mistake blanked the player's browse page twice [REQ-LIB-165].
    if (d && d.error) throw new Error(d.error);
    return d;
  },

  n(x) { return (x ?? 0).toLocaleString(); },

  clock(ms) {
    const t = Math.round((ms || 0) / 1000);
    return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`;
  },

  el(tag, attrs = {}, ...kids) {
    const e = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs)) {
      if (v === null || v === undefined || v === false) continue;
      if (k === 'class') e.className = v;
      else if (k === 'text') e.textContent = v;
      else e.setAttribute(k, v);
    }
    for (const k of kids) if (k !== null && k !== undefined) e.append(k);
    return e;
  },

  // An id that is not an MBID is not a defect -- it means no MusicBrainz entry
  // exists for this audio [IMPL-SUI-025]. Say that, rather than flagging it red.
  isMbid(s) {
    return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(s || '');
  },

  fail(where, e) {
    const box = document.querySelector(where);
    if (box) box.replaceChildren(S.el('p', { class: 'err', text: String(e.message || e) }));
  },
};
