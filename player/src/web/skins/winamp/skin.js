// The awkward case, on purpose: a fixed-width appliance rather than a document,
// with its own type, geometry and a scrolling title. It needed nothing from
// core.js that the other two did not -- which is what makes the contract worth
// having rather than merely tidy.
(() => {
  const $ = id => document.getElementById(id);
  const { clock, overlap } = Vaino.fmt;

  const showVolume = Vaino.bindVolume($('volume'), $('volnum'));
  const showProgram = Vaino.bindProgram($('prog'), 'auto (by clock)');
  const showQueue = Vaino.bindQueue(
    $('queue'), q => (q.artist ? `${q.artist} - ${q.title}` : q.title));
  const skipFade = $('skipfade'), skipLead = $('skiplead');
  skipFade.onchange = () => Vaino.skipFade(skipFade.value * 1000);
  skipLead.onchange = () => Vaino.skipLead(skipLead.value * 1000);

  // Scroll only when it will not fit. A title that fits and scrolls anyway is
  // the habit that made these players tiring to watch.
  let shown = null;
  function renderTitle(text) {
    if (text === shown) return;
    shown = text;
    const m = $('marquee');
    m.classList.remove('roll');
    m.textContent = '';
    const span = document.createElement('span');
    span.id = 'title';
    span.textContent = text;
    m.appendChild(span);
    if (span.scrollWidth > m.clientWidth) {
      // Doubling the text is what makes the wrap seamless at -50%.
      span.textContent = text + '   ';
      m.appendChild(span.cloneNode(true));
      m.classList.add('roll');
    }
  }

  let progSig = '';


  Vaino.subscribe(s => {
    // "Artist - Title" is this idiom's own habit, and it happens to put the
    // more identifying half first when the line has to scroll.
    renderTitle(s.artist ? `${s.artist} - ${s.title}` : (s.title ?? '—'));
    Vaino.showArt($('art'), s.passage_id);
    $('time').textContent = clock(s.position_ms);
    $('stat').textContent =
      `${clock(s.duration_ms)} · ${s.queue_len ?? 0} queued` +
      (s.plays ? ` · ${s.plays} plays` : '') +
      (s.underrun_samples ? ` · ${s.underrun_samples} under` : '') + ' ';
    // Provenance rides in the stat row rather than the marquee: the marquee
    // scrolls and is cloned to make the wrap seamless, so a badge in there
    // would drift off the panel and then appear twice `[REQ-VIS-120]`. Only
    // the names this skin actually shows are qualified -- it has no album
    // line, so an album badge would be marking something invisible.
    $('stat').appendChild(Vaino.badge(s.title_source, 'title'));
    if (s.artist) $('stat').appendChild(Vaino.badge(s.artist_source, 'artist'));
    $('state').textContent =
      Vaino.status === 'connected' ? (s.playing ? '▶ playing' : '❚❚ paused')
                                   : '… reconnecting';
    $('fill').style.width =
      s.duration_ms ? `${(s.position_ms / s.duration_ms) * 100}%` : '0';
    showVolume(s);
    showProgram(s);
    showQueue(s);
    if (s.skip) {
      if (document.activeElement !== skipFade) {
        skipFade.min = 0;
        skipFade.max = s.skip.fade_max_ms / 1000;
        skipFade.value = (s.skip.fade_ms / 1000).toFixed(1);
      }
      if (document.activeElement !== skipLead) {
        skipLead.min = s.skip.lead_min_ms / 1000;
        skipLead.max = s.skip.lead_max_ms / 1000;
        skipLead.value = (s.skip.lead_ms / 1000).toFixed(1);
      }
      $('skipoverlap').textContent = overlap(s.skip);
    }
    $('why').textContent = s.why
      ? `${s.why.program ? s.why.program + ': ' : ''}` +
        `${s.why.share_pct.toFixed(2)}% of ${s.why.pool_size.toLocaleString()} ` +
        `candidates, rank #${s.why.rank + 1}`
      : '';
  });
})();
