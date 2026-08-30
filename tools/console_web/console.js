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
    // `where` is a CSS selector everywhere this was first written, but a
    // caller that already holds the element (a per-candidate result box
    // with no id of its own, e.g.) has no selector to give it -- accepting
    // either avoids inventing one just to satisfy this signature.
    const box = typeof where === 'string' ? document.querySelector(where) : where;
    if (box) box.replaceChildren(S.el('p', { class: 'err', text: String(e.message || e) }));
  },

  // A live job's log while it runs, then whatever the caller wants once it
  // stops -- factored out of flags.html's own watchSync()/showResult(),
  // which a second job-launching page (profile.html's reanalyze button) was
  // about to duplicate rather than share [SPEC-SUI-214].
  watchJob(id, box, onDone) {
    const log = S.el('div', { class: 'list' });
    box.replaceChildren(log);
    const es = new EventSource(`/api/jobs/${id}/stream`);
    es.onmessage = async m => {
      const e = JSON.parse(m.data);
      if (e.kind === 'counts') return;
      const line = e.kind === 'stage' ? `── ${e.text} ──`
        : e.kind === 'done' ? `── finished: ${e.text} ──` : e.text;
      log.append(S.el('div', {
        style: e.kind === 'error' ? 'color:var(--bad)'
          : (e.kind === 'stage' || e.kind === 'done') ? 'color:var(--accent)' : '',
        text: line,
      }));
      log.scrollTop = log.scrollHeight;
      if (e.kind === 'done') {
        es.close();
        const job = await S.get(`/api/jobs/${id}`);
        if (onDone) onDone(job, box);
      }
    };
  },

  // The default `onDone`: flat numeric tiles -- what most job results
  // already are (flags pull/push's matched/already/unmatched, and so on).
  // A caller whose `result` is structured differently passes its own
  // `onDone` to `watchJob` instead of using this.
  tileResult(job, box) {
    if (!job.result) return;
    box.append(S.el('div', { class: 'tiles' },
      ...Object.entries(job.result).map(([k, v]) =>
        S.el('div', { class: 'tile' }, S.el('b', { text: S.n(v) }), S.el('span', { text: k })))));
  },
};
