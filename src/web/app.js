// Vaino Web UI Application Client

let ws = null;
let currentStatus = null;
let tracksList = [];

// DOM Elements
const connectionBadge = document.getElementById('connection-badge');
const connectionText = document.getElementById('connection-text');
const statusDot = connectionBadge.querySelector('.status-dot');

const albumArt = document.getElementById('album-art');
const trackStatusPill = document.getElementById('track-status-pill');
const trackTitle = document.getElementById('track-title');
const trackArtist = document.getElementById('track-artist');
const trackAlbum = document.getElementById('track-album');

const timeElapsed = document.getElementById('time-elapsed');
const timeTotal = document.getElementById('time-total');
const progressBarFill = document.getElementById('progress-bar-fill');
const visualizerWave = document.getElementById('visualizer-wave');

const btnPlayPause = document.getElementById('btn-play-pause');
const iconPlay = document.getElementById('icon-play');
const iconPause = document.getElementById('icon-pause');
const btnSkip = document.getElementById('btn-skip');

const volumeSlider = document.getElementById('volume-slider');
const volumeValue = document.getElementById('volume-value');

const libraryTbody = document.getElementById('library-tbody');
const librarySearch = document.getElementById('library-search');

// Format milliseconds to mm:ss
function formatTime(ms) {
    if (!ms || isNaN(ms)) return '0:00';
    const totalSeconds = Math.floor(ms / 1000);
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = totalSeconds % 60;
    return `${minutes}:${seconds < 10 ? '0' : ''}${seconds}`;
}

// WebSocket Setup
function initWebSocket() {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/ws`;

    ws = new WebSocket(wsUrl);

    ws.onopen = () => {
        statusDot.classList.add('connected');
        connectionText.textContent = 'Live Connected';
    };

    ws.onmessage = (event) => {
        try {
            const payload = JSON.parse(event.data);
            if (payload.type === 'STATUS_UPDATE') {
                updateUI(payload.data);
            }
        } catch (e) {
            console.error('Error parsing WebSocket message:', e);
        }
    };

    ws.onclose = () => {
        statusDot.classList.remove('connected');
        connectionText.textContent = 'Disconnected';
        setTimeout(initWebSocket, 3000);
    };

    ws.onerror = (err) => {
        console.error('WebSocket error:', err);
    };
}

// Update UI State from Status Data
function updateUI(status) {
    currentStatus = status;

    trackStatusPill.textContent = status.state;
    volumeSlider.value = status.volume;
    volumeValue.textContent = `${status.volume}%`;

    const queueBadge = document.getElementById('queue-badge');
    if (queueBadge) queueBadge.textContent = status.queue_length || 0;

    const btnPrevTrack = document.getElementById('btn-prev');
    if (btnPrevTrack) {
        btnPrevTrack.disabled = !status.can_skip_back;
        btnPrevTrack.style.opacity = status.can_skip_back ? '1' : '0.4';
    }

    if (status.state === 'PLAYING') {
        iconPlay.style.display = 'none';
        iconPause.style.display = 'block';
        visualizerWave.classList.add('active');
    } else {
        iconPlay.style.display = 'block';
        iconPause.style.display = 'none';
        visualizerWave.classList.remove('active');
    }

    if (status.current_track) {
        const t = status.current_track;
        trackTitle.textContent = t.title || 'Unknown Title';
        trackArtist.textContent = t.artist || 'Unknown Artist';
        trackAlbum.textContent = t.album || 'Vaino Station Engine';

        timeElapsed.textContent = formatTime(status.elapsed_ms);
        timeTotal.textContent = formatTime(status.duration_ms);

        const progressPercent = status.duration_ms > 0 ? (status.elapsed_ms / status.duration_ms) * 100 : 0;
        progressBarFill.style.width = `${Math.min(100, progressPercent)}%`;

        if (t.has_cover_art) {
            albumArt.src = `/api/v1/art/${t.id}`;
        } else {
            albumArt.src = "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='300' height='300' viewBox='0 0 300 300'><rect width='300' height='300' fill='%231e2230'/><text x='50%' y='50%' dominant-baseline='middle' text-anchor='middle' fill='%234a5568' font-size='48'>🎵</text></svg>";
        }
    } else {
        trackTitle.textContent = 'No Track Selected';
        trackArtist.textContent = 'Select a track to start playback';
        trackAlbum.textContent = 'Vaino Station Engine';
        timeElapsed.textContent = '0:00';
        timeTotal.textContent = '0:00';
        progressBarFill.style.width = '0%';
    }

    renderQueue(status.current_track, status.queue || []);
}

// Library Pagination & View State
let currentView = 'tracks'; // 'tracks', 'artists', 'albums'
let currentPage = 1;
let tracksPageSize = 50;
let totalTracks = 0;
let currentQuery = '';
let currentLetter = '';
let currentArtistFilter = '';
let currentAlbumFilter = '';

let currentArtistPage = 1;
let artistsPageSize = 50;
let totalArtists = 0;

let currentAlbumPage = 1;
let albumsPageSize = 50;
let totalAlbums = 0;

// Fetch & Render Library Tracks
async function fetchLibrary(page = 1) {
    if (currentView === 'artists' && !currentArtistFilter && !currentAlbumFilter) return fetchArtists(page);
    if (currentView === 'albums' && !currentArtistFilter && !currentAlbumFilter) return fetchAlbums(null, page);

    currentPage = page;
    const offset = (currentPage - 1) * tracksPageSize;

    try {
        let url = `/api/v1/library/tracks?limit=${tracksPageSize}&offset=${offset}`;
        if (currentArtistFilter) {
            url += `&artist=${encodeURIComponent(currentArtistFilter)}`;
        }
        if (currentAlbumFilter) {
            url += `&album=${encodeURIComponent(currentAlbumFilter)}`;
        }
        if (currentLetter) {
            url += `&letter=${encodeURIComponent(currentLetter)}`;
        }
        if (currentQuery) {
            url += `&query=${encodeURIComponent(currentQuery)}`;
        }
        const res = await fetch(url);
        const data = await res.json();
        
        tracksList = data.tracks;
        totalTracks = data.total || 0;
        
        renderLibrary(tracksList, offset);
        updatePaginationControls();
    } catch (e) {
        console.error('Error fetching library:', e);
        libraryTbody.innerHTML = `<tr><td colspan="6" class="loading-cell">Failed loading library tracks.</td></tr>`;
    }
}

// Fetch & Render Artists Grid [REQ-UI-020A, REQ-UI-020E, REQ-UI-020I]
async function fetchArtists(page = 1) {
    const grid = document.getElementById('artists-grid');
    if (!grid) return;
    grid.innerHTML = `<div class="loading-cell">Loading artists...</div>`;

    currentArtistPage = page;
    const offset = (currentArtistPage - 1) * artistsPageSize;

    try {
        let url = `/api/v1/library/artists?limit=${artistsPageSize}&offset=${offset}`;
        if (currentLetter) {
            url += `&letter=${encodeURIComponent(currentLetter)}`;
        }
        if (currentQuery) {
            url += `&query=${encodeURIComponent(currentQuery)}`;
        }
        const res = await fetch(url);
        const data = await res.json();
        const artists = data.artists || [];
        totalArtists = data.total || 0;

        const countBadge = document.getElementById('library-count-badge');
        if (countBadge) countBadge.textContent = `${totalArtists.toLocaleString()} artists`;

        if (artists.length === 0) {
            grid.innerHTML = `<div class="loading-cell">No artists found.</div>`;
        } else {
            grid.innerHTML = artists.map(a => `
                <div class="nav-card" onclick="browseArtistAlbums('${escapeHtml(a.artist)}')">
                    <img src="/api/v1/art/${a.sample_track_id}" class="nav-card-art" onerror="this.src='data:image/svg+xml;utf8,<svg xmlns=\\'http://www.w3.org/2000/svg\\' width=\\'140\\' height=\\'140\\'><rect width=\\'140\\' height=\\'140\\' fill=\\'%231e2230\\'/><text x=\\'50%\\' y=\\'50%\\' dominant-baseline=\\'middle\\' text-anchor=\\'middle\\' fill=\\'%234a5568\\' font-size=\\'36\\'>🎙️</text></svg>'">
                    <div class="nav-card-title">${escapeHtml(a.artist)}</div>
                    <div class="nav-card-subtitle">${a.album_count} Albums • ${a.track_count} Tracks</div>
                    <span class="nav-card-badge">View Albums ▶</span>
                </div>
            `).join('');
        }

        updateArtistsPagination(totalArtists, offset, artists.length);
    } catch (e) {
        console.error('Error fetching artists:', e);
        grid.innerHTML = `<div class="loading-cell">Failed loading artists.</div>`;
    }
}

function updateArtistsPagination(total, offset, count) {
    const info = document.getElementById('artists-pagination-info');
    const indicator = document.getElementById('artists-page-indicator');
    const btnFirst = document.getElementById('btn-first-artists');
    const btnPrev = document.getElementById('btn-prev-artists');
    const btnNext = document.getElementById('btn-next-artists');
    const btnLast = document.getElementById('btn-last-artists');

    const totalPages = Math.max(1, Math.ceil(total / artistsPageSize));
    const start = total === 0 ? 0 : offset + 1;
    const end = offset + count;

    if (info) info.textContent = `Showing ${start.toLocaleString()}–${end.toLocaleString()} of ${total.toLocaleString()} artists`;
    if (indicator) indicator.textContent = `Page ${currentArtistPage} of ${totalPages}`;

    const hasMultiplePages = total > artistsPageSize;

    if (btnFirst) btnFirst.style.display = (hasMultiplePages && currentArtistPage > 1) ? 'inline-block' : 'none';
    if (btnPrev) btnPrev.style.display = (hasMultiplePages && currentArtistPage > 1) ? 'inline-block' : 'none';
    if (btnNext) btnNext.style.display = (hasMultiplePages && currentArtistPage < totalPages) ? 'inline-block' : 'none';
    if (btnLast) btnLast.style.display = (hasMultiplePages && currentArtistPage < totalPages) ? 'inline-block' : 'none';
}

// Fetch & Render Albums Grid [REQ-UI-020A, REQ-UI-020D, REQ-UI-020E, REQ-UI-020I]
async function fetchAlbums(artistFilter = null, page = 1) {
    const grid = document.getElementById('albums-grid');
    if (!grid) return;
    grid.innerHTML = `<div class="loading-cell">Loading albums...</div>`;

    currentAlbumPage = page;
    const offset = (currentAlbumPage - 1) * albumsPageSize;
    const activeArtist = artistFilter || currentArtistFilter;

    try {
        let url = `/api/v1/library/albums?limit=${albumsPageSize}&offset=${offset}`;
        if (activeArtist) {
            url += `&artist=${encodeURIComponent(activeArtist)}`;
        }
        if (currentLetter) {
            url += `&letter=${encodeURIComponent(currentLetter)}`;
        }
        if (currentQuery) {
            url += `&query=${encodeURIComponent(currentQuery)}`;
        }
        const res = await fetch(url);
        const data = await res.json();
        const albums = data.albums || [];
        totalAlbums = data.total || 0;

        renderAlbumsGrid(albums, activeArtist, totalAlbums);
        updateAlbumsPagination(totalAlbums, offset, albums.length);
    } catch (e) {
        console.error('Error fetching albums:', e);
        grid.innerHTML = `<div class="loading-cell">Failed loading albums.</div>`;
    }
}

function updateAlbumsPagination(total, offset, count) {
    const info = document.getElementById('albums-pagination-info');
    const indicator = document.getElementById('albums-page-indicator');
    const btnFirst = document.getElementById('btn-first-albums');
    const btnPrev = document.getElementById('btn-prev-albums');
    const btnNext = document.getElementById('btn-next-albums');
    const btnLast = document.getElementById('btn-last-albums');

    const totalPages = Math.max(1, Math.ceil(total / albumsPageSize));
    const start = total === 0 ? 0 : offset + 1;
    const end = offset + count;

    if (info) info.textContent = `Showing ${start.toLocaleString()}–${end.toLocaleString()} of ${total.toLocaleString()} albums`;
    if (indicator) indicator.textContent = `Page ${currentAlbumPage} of ${totalPages}`;

    const hasMultiplePages = total > albumsPageSize;

    if (btnFirst) btnFirst.style.display = (hasMultiplePages && currentAlbumPage > 1) ? 'inline-block' : 'none';
    if (btnPrev) btnPrev.style.display = (hasMultiplePages && currentAlbumPage > 1) ? 'inline-block' : 'none';
    if (btnNext) btnNext.style.display = (hasMultiplePages && currentAlbumPage < totalPages) ? 'inline-block' : 'none';
    if (btnLast) btnLast.style.display = (hasMultiplePages && currentAlbumPage < totalPages) ? 'inline-block' : 'none';
}

// Drill-down from Artist card to their Albums [REQ-UI-020C, REQ-UI-020D]
async function browseArtistAlbums(artistName) {
    currentArtistFilter = artistName;
    currentLetter = '';
    document.querySelectorAll('.letter-btn').forEach(b => b.classList.remove('active'));
    const allLetterBtn = document.querySelector('.letter-btn[data-letter=""]');
    if (allLetterBtn) allLetterBtn.classList.add('active');

    try {
        const url = `/api/v1/library/albums?artist=${encodeURIComponent(artistName)}&limit=100`;
        const res = await fetch(url);
        const data = await res.json();
        const albums = data.albums || [];
        const total = data.total || albums.length;

        if (albums.length === 1) {
            openAlbumTracklist(albums[0].album);
        } else {
            switchView('albums');
            renderAlbumsGrid(albums, artistName, total);
            updateAlbumsPagination(total, 0, albums.length);
            setBreadcrumbFilter('Artist', artistName);
        }
    } catch (e) {
        console.error('Error browsing artist albums:', e);
    }
}

function renderAlbumsGrid(albums, artistFilter = null, totalCount = null) {
    const grid = document.getElementById('albums-grid');
    if (!grid) return;

    const displayTotal = totalCount !== null ? totalCount : albums.length;
    const countBadge = document.getElementById('library-count-badge');
    if (countBadge) {
        countBadge.textContent = artistFilter ? `${artistFilter} (${displayTotal} albums)` : `${displayTotal} albums`;
    }

    if (albums.length === 0) {
        grid.innerHTML = `<div class="loading-cell">No albums found.</div>`;
        return;
    }

    grid.innerHTML = albums.map(al => `
        <div class="nav-card album-card" data-album="${escapeHtml(al.album)}">
            <img src="/api/v1/art/${al.sample_track_id}" class="nav-card-art" onerror="this.src='data:image/svg+xml;utf8,<svg xmlns=\\'http://www.w3.org/2000/svg\\' width=\\'140\\' height=\\'140\\'><rect width=\\'140\\' height=\\'140\\' fill=\\'%231e2230\\'/><text x=\\'50%\\' y=\\'50%\\' dominant-baseline=\\'middle\\' text-anchor=\\'middle\\' fill=\\'%234a5568\\' font-size=\\'36\\'>💿</text></svg>'">
            <div class="nav-card-title">${escapeHtml(al.album)}</div>
            <div class="nav-card-subtitle">${escapeHtml(al.artist)} ${al.year ? '(' + al.year + ')' : ''}</div>
            <div class="table-action-group" style="margin-top: 6px;">
                <span class="nav-card-badge">${al.track_count} Tracks ▶</span>
                <button class="btn-action-sm btn-album-next" data-album="${escapeHtml(al.album)}" title="Enqueue Album Next">➕ Next</button>
                <button class="btn-action-sm btn-album-add" data-album="${escapeHtml(al.album)}" title="Add Album to Queue">📥 Queue</button>
            </div>
        </div>
    `).join('');
}

// Drill-down from Album card to sorted Tracklist [REQ-UI-020B, REQ-UI-020D, REQ-UI-020H]
async function openAlbumTracklist(albumName) {
    currentAlbumFilter = albumName;
    currentArtistFilter = ''; // Clear artist filter to show full album context!
    
    currentLetter = '';
    document.querySelectorAll('.letter-btn').forEach(b => b.classList.remove('active'));
    const allLetterBtn = document.querySelector('.letter-btn[data-letter=""]');
    if (allLetterBtn) allLetterBtn.classList.add('active');

    switchView('tracks');
    try {
        const url = `/api/v1/library/albums/${encodeURIComponent(albumName)}/tracks`;
        const res = await fetch(url);
        const data = await res.json();
        
        tracksList = data.tracks || [];
        totalTracks = tracksList.length;
        renderLibrary(tracksList, 0);
        updatePaginationControls();

        const countBadge = document.getElementById('library-count-badge');
        if (countBadge) countBadge.textContent = `${albumName} (${totalTracks} tracks)`;

        setBreadcrumbFilter('Album', albumName);
    } catch (e) {
        console.error('Error fetching album tracklist:', e);
    }
}

// Breadcrumb Filter Management [REQ-UI-020D]
function setBreadcrumbFilter(type, value) {
    const bar = document.getElementById('filter-breadcrumb-bar');
    const text = document.getElementById('breadcrumb-text');
    if (bar && text) {
        text.textContent = `${type}: ${value}`;
        bar.style.display = 'flex';
    }
}

function clearBreadcrumbFilter() {
    currentArtistFilter = '';
    currentAlbumFilter = '';
    currentQuery = '';
    currentLetter = '';

    const searchInput = document.getElementById('library-search');
    if (searchInput) searchInput.value = '';

    document.querySelectorAll('.letter-btn').forEach(b => b.classList.remove('active'));
    const allLetterBtn = document.querySelector('.letter-btn[data-letter=""]');
    if (allLetterBtn) allLetterBtn.classList.add('active');

    const bar = document.getElementById('filter-breadcrumb-bar');
    if (bar) bar.style.display = 'none';

    if (currentView === 'tracks') fetchLibrary(1);
    else if (currentView === 'albums') fetchAlbums();
    else if (currentView === 'artists') fetchArtists();
}

// View Tab Switcher
function switchView(targetView) {
    currentView = targetView;
    document.querySelectorAll('.view-tab').forEach(b => {
        b.classList.toggle('active', b.getAttribute('data-view') === targetView);
    });

    document.querySelectorAll('.view-content-block').forEach(el => el.style.display = 'none');
    const targetBlock = document.getElementById(`view-container-${targetView}`);
    if (targetBlock) targetBlock.style.display = 'block';

    if (targetView === 'tracks') fetchLibrary(1);
    else if (targetView === 'artists') fetchArtists();
    else if (targetView === 'albums') fetchAlbums(currentArtistFilter);
}

function renderLibrary(tracks, offset = 0) {
    if (!tracks || tracks.length === 0) {
        libraryTbody.innerHTML = `<tr><td colspan="6" class="loading-cell">No tracks found in library.</td></tr>`;
        return;
    }

    libraryTbody.innerHTML = tracks.map((t, idx) => `
        <tr class="track-row" data-id="${t.id}">
            <td>${offset + idx + 1}</td>
            <td><strong>${escapeHtml(t.title)}</strong></td>
            <td>${escapeHtml(t.artist)}</td>
            <td>${escapeHtml(t.album || '-')}</td>
            <td>${formatTime(t.duration_ms)}</td>
            <td>
                <div class="table-action-group">
                    <button class="btn-action-sm btn-play-now" data-id="${t.id}" title="Play Now">▶ Play</button>
                    <button class="btn-action-sm btn-enqueue-next" data-id="${t.id}" title="Play Next">➕ Next</button>
                    <button class="btn-action-sm btn-enqueue-add" data-id="${t.id}" title="Add to Queue">📥 Queue</button>
                </div>
            </td>
        </tr>
    `).join('');
}

function updatePaginationControls() {
    const countBadge = document.getElementById('library-count-badge');
    const paginationInfo = document.getElementById('tracks-pagination-info');
    const pageIndicator = document.getElementById('tracks-page-indicator');
    const btnFirst = document.getElementById('btn-first-tracks');
    const btnPrev = document.getElementById('btn-prev-tracks');
    const btnNext = document.getElementById('btn-next-tracks');
    const btnLast = document.getElementById('btn-last-tracks');

    const totalPages = Math.max(1, Math.ceil(totalTracks / tracksPageSize));
    const startItem = totalTracks === 0 ? 0 : (currentPage - 1) * tracksPageSize + 1;
    const endItem = Math.min(totalTracks, currentPage * tracksPageSize);

    if (countBadge) countBadge.textContent = `${totalTracks.toLocaleString()} tracks`;
    if (paginationInfo) paginationInfo.textContent = `Showing ${startItem.toLocaleString()}–${endItem.toLocaleString()} of ${totalTracks.toLocaleString()} tracks`;
    if (pageIndicator) pageIndicator.textContent = `Page ${currentPage} of ${totalPages}`;

    const hasMultiplePages = totalTracks > tracksPageSize;

    if (btnFirst) btnFirst.style.display = (hasMultiplePages && currentPage > 1) ? 'inline-block' : 'none';
    if (btnPrev) btnPrev.style.display = (hasMultiplePages && currentPage > 1) ? 'inline-block' : 'none';
    if (btnNext) btnNext.style.display = (hasMultiplePages && currentPage < totalPages) ? 'inline-block' : 'none';
    if (btnLast) btnLast.style.display = (hasMultiplePages && currentPage < totalPages) ? 'inline-block' : 'none';
}

function escapeHtml(str) {
    if (!str) return '';
    return str.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

// Actions
async function playTrack(trackId) {
    try {
        await fetch(`/api/v1/player/play?track_id=${trackId}`, { method: 'POST' });
    } catch (e) {
        console.error('Error playing track:', e);
    }
}

btnPlayPause.addEventListener('click', async () => {
    if (!currentStatus) return;
    const method = 'POST';
    if (currentStatus.state === 'PLAYING') {
        await fetch('/api/v1/player/pause', { method });
    } else {
        await fetch('/api/v1/player/play', { method });
    }
});

btnSkip.addEventListener('click', async () => {
    try {
        const res = await fetch('/api/v1/player/skip', { method: 'POST' });
        if (res.ok) {
            const status = await res.json();
            updateUI(status);
        }
    } catch (e) {
        console.error('Error skipping track:', e);
    }
});

volumeSlider.addEventListener('input', (e) => {
    const vol = parseInt(e.target.value);
    volumeValue.textContent = `${vol}%`;
    if (ws && ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ action: 'VOLUME', volume: vol }));
    }
});

let searchTimeout;
librarySearch.addEventListener('input', (e) => {
    clearTimeout(searchTimeout);
    currentQuery = e.target.value;
    searchTimeout = setTimeout(() => {
        fetchLibrary(1);
    }, 250);
});

// --- Tracks Pagination Listeners ---
const tracksPageSizeSelect = document.getElementById('tracks-page-size-select');
if (tracksPageSizeSelect) {
    tracksPageSizeSelect.addEventListener('change', (e) => {
        tracksPageSize = parseInt(e.target.value);
        fetchLibrary(1);
    });
}
const btnFirstTracks = document.getElementById('btn-first-tracks');
const btnPrevTracks = document.getElementById('btn-prev-tracks');
const btnNextTracks = document.getElementById('btn-next-tracks');
const btnLastTracks = document.getElementById('btn-last-tracks');

if (btnFirstTracks) btnFirstTracks.addEventListener('click', () => fetchLibrary(1));
if (btnPrevTracks) btnPrevTracks.addEventListener('click', () => { if (currentPage > 1) fetchLibrary(currentPage - 1); });
if (btnNextTracks) btnNextTracks.addEventListener('click', () => fetchLibrary(currentPage + 1));
if (btnLastTracks) btnLastTracks.addEventListener('click', () => fetchLibrary(Math.ceil(totalTracks / tracksPageSize)));

// --- Artists Pagination Listeners ---
const artistsPageSizeSelect = document.getElementById('artists-page-size-select');
if (artistsPageSizeSelect) {
    artistsPageSizeSelect.addEventListener('change', (e) => {
        artistsPageSize = parseInt(e.target.value);
        fetchArtists(1);
    });
}
const btnFirstArtists = document.getElementById('btn-first-artists');
const btnPrevArtists = document.getElementById('btn-prev-artists');
const btnNextArtists = document.getElementById('btn-next-artists');
const btnLastArtists = document.getElementById('btn-last-artists');

if (btnFirstArtists) btnFirstArtists.addEventListener('click', () => fetchArtists(1));
if (btnPrevArtists) btnPrevArtists.addEventListener('click', () => { if (currentArtistPage > 1) fetchArtists(currentArtistPage - 1); });
if (btnNextArtists) btnNextArtists.addEventListener('click', () => fetchArtists(currentArtistPage + 1));
if (btnLastArtists) btnLastArtists.addEventListener('click', () => fetchArtists(Math.ceil(totalArtists / artistsPageSize)));

// --- Albums Pagination Listeners ---
const albumsPageSizeSelect = document.getElementById('albums-page-size-select');
if (albumsPageSizeSelect) {
    albumsPageSizeSelect.addEventListener('change', (e) => {
        albumsPageSize = parseInt(e.target.value);
        fetchAlbums(null, 1);
    });
}
const btnFirstAlbums = document.getElementById('btn-first-albums');
const btnPrevAlbums = document.getElementById('btn-prev-albums');
const btnNextAlbums = document.getElementById('btn-next-albums');
const btnLastAlbums = document.getElementById('btn-last-albums');

if (btnFirstAlbums) btnFirstAlbums.addEventListener('click', () => fetchAlbums(null, 1));
if (btnPrevAlbums) btnPrevAlbums.addEventListener('click', () => { if (currentAlbumPage > 1) fetchAlbums(null, currentAlbumPage - 1); });
if (btnNextAlbums) btnNextAlbums.addEventListener('click', () => fetchAlbums(null, currentAlbumPage + 1));
if (btnLastAlbums) btnLastAlbums.addEventListener('click', () => fetchAlbums(null, Math.ceil(totalAlbums / albumsPageSize)));

// Breadcrumb Filter Clear Listener [REQ-UI-020D]
const btnClearBreadcrumb = document.getElementById('btn-clear-breadcrumb');
if (btnClearBreadcrumb) {
    btnClearBreadcrumb.addEventListener('click', () => {
        clearBreadcrumbFilter();
    });
}

// View Tabs Navigation Listener [REQ-UI-020A, REQ-UI-020H]
const viewTabs = document.getElementById('view-tabs');
if (viewTabs) {
    viewTabs.addEventListener('click', (e) => {
        if (e.target.classList.contains('view-tab')) {
            const targetView = e.target.getAttribute('data-view');
            if (targetView) {
                clearBreadcrumbFilter();
                switchView(targetView);
            }
        }
    });
}

// Letter Bar Navigation Listener
const letterNavBar = document.getElementById('letter-nav-bar');
if (letterNavBar) {
    letterNavBar.addEventListener('click', (e) => {
        if (e.target.classList.contains('letter-btn')) {
            document.querySelectorAll('.letter-btn').forEach(b => b.classList.remove('active'));
            e.target.classList.add('active');
            currentLetter = e.target.getAttribute('data-letter') || '';
            fetchLibrary(1);
        }
    });
}

// Initialize Clock Widget
function startClock() {
    const clockEl = document.getElementById('clock-widget');
    if (!clockEl) return;
    function updateClock() {
        const now = new Date();
        clockEl.textContent = now.toLocaleTimeString();
    }
    updateClock();
    setInterval(updateClock, 1000);
}

// Kiosk / Wall Art Mode Toggle
const kioskBtn = document.getElementById('kiosk-toggle-btn');
if (kioskBtn) {
    kioskBtn.addEventListener('click', () => {
        document.body.classList.toggle('kiosk-mode');
        if (document.body.classList.contains('kiosk-mode')) {
            if (document.documentElement.requestFullscreen) {
                document.documentElement.requestFullscreen().catch(() => {});
            }
        } else {
            if (document.exitFullscreen) {
                document.exitFullscreen().catch(() => {});
            }
        }
    });
}

// --- Playlist Queue Management UI [REQ-QUE-010, REQ-QUE-020, REQ-QUE-030, REQ-QUE-040] ---

let isQueueOpen = false;
let currentQueuePage = 1;
let queuePageSize = 50;

function toggleQueueDrawer(show = null) {
    const drawer = document.getElementById('queue-drawer');
    const overlay = document.getElementById('queue-drawer-overlay');
    if (!drawer || !overlay) return;

    if (show === null) isQueueOpen = !isQueueOpen;
    else isQueueOpen = show;

    drawer.style.display = isQueueOpen ? 'flex' : 'none';
    overlay.style.display = isQueueOpen ? 'block' : 'none';

    if (isQueueOpen && currentStatus) {
        renderQueue(currentStatus.current_track, currentStatus.queue || []);
    }
}

function renderQueue(currentTrack, queueList) {
    const subtitle = document.getElementById('queue-drawer-subtitle');
    const nowPlayingContainer = document.getElementById('queue-now-playing');
    const queueListContainer = document.getElementById('queue-items-list');

    const totalQueued = queueList ? queueList.length : 0;
    if (subtitle) subtitle.textContent = `${totalQueued} track${totalQueued === 1 ? '' : 's'} in queue`;

    if (nowPlayingContainer) {
        if (currentTrack) {
            const artSrc = currentTrack.has_cover_art ? `/api/v1/art/${currentTrack.id}` : "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='140' height='140'><rect width='140' height='140' fill='%231e2230'/><text x='50%' y='50%' dominant-baseline='middle' text-anchor='middle' fill='%234a5568' font-size='36'>🎵</text></svg>";
            nowPlayingContainer.innerHTML = `
                <img src="${artSrc}" class="now-playing-card-art">
                <div class="now-playing-card-info">
                    <div class="now-playing-card-title">${escapeHtml(currentTrack.title)}</div>
                    <div class="now-playing-card-artist">${escapeHtml(currentTrack.artist)} • ${escapeHtml(currentTrack.album || '')}</div>
                </div>
            `;
        } else {
            nowPlayingContainer.innerHTML = `<div class="loading-cell">No track currently playing.</div>`;
        }
    }

    if (!queueListContainer) return;

    if (!queueList || queueList.length === 0) {
        queueListContainer.innerHTML = `<div class="loading-cell">Queue is empty.</div>`;
        updateQueuePagination(0);
        return;
    }

    const totalPages = Math.max(1, Math.ceil(totalQueued / queuePageSize));
    if (currentQueuePage > totalPages) currentQueuePage = totalPages;
    if (currentQueuePage < 1) currentQueuePage = 1;

    const startIdx = (currentQueuePage - 1) * queuePageSize;
    const endIdx = Math.min(totalQueued, startIdx + queuePageSize);
    const pagedItems = queueList.slice(startIdx, endIdx);

    queueListContainer.innerHTML = pagedItems.map((item, localIdx) => {
        const actualIdx = startIdx + localIdx;
        const artSrc = item.has_cover_art ? `/api/v1/art/${item.id}` : "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='140' height='140'><rect width='140' height='140' fill='%231e2230'/><text x='50%' y='50%' dominant-baseline='middle' text-anchor='middle' fill='%234a5568' font-size='24'>🎵</text></svg>";
        const isFirst = actualIdx === 0;
        const isLast = actualIdx === totalQueued - 1;

        return `
            <div class="queue-item-card">
                <div class="queue-item-details">
                    <img src="${artSrc}" class="queue-item-art">
                    <div class="queue-item-text">
                        <div class="queue-item-title">${actualIdx + 1}. ${escapeHtml(item.title)}</div>
                        <div class="queue-item-artist">${escapeHtml(item.artist)}</div>
                    </div>
                </div>
                <div class="queue-item-actions">
                    ${!isFirst ? `<button class="btn-queue-action" onclick="moveQueueItem(${actualIdx}, ${actualIdx - 1})" title="Move Up">▲</button>` : ''}
                    ${!isLast ? `<button class="btn-queue-action" onclick="moveQueueItem(${actualIdx}, ${actualIdx + 1})" title="Move Down">▼</button>` : ''}
                    <button class="btn-queue-action btn-queue-delete" onclick="removeQueueItem(${actualIdx})" title="Remove from Queue">🗑</button>
                </div>
            </div>
        `;
    }).join('');

    updateQueuePagination(totalQueued);
}

function updateQueuePagination(totalQueued) {
    const info = document.getElementById('queue-pagination-info');
    const indicator = document.getElementById('queue-page-indicator');
    const btnFirst = document.getElementById('btn-first-queue');
    const btnPrev = document.getElementById('btn-prev-queue');
    const btnNext = document.getElementById('btn-next-queue');
    const btnLast = document.getElementById('btn-last-queue');

    const totalPages = Math.max(1, Math.ceil(totalQueued / queuePageSize));
    const startItem = totalQueued === 0 ? 0 : (currentQueuePage - 1) * queuePageSize + 1;
    const endItem = Math.min(totalQueued, currentQueuePage * queuePageSize);

    if (info) info.textContent = `Showing ${startItem.toLocaleString()}–${endItem.toLocaleString()} of ${totalQueued.toLocaleString()} queued tracks`;
    if (indicator) indicator.textContent = `Page ${currentQueuePage} of ${totalPages}`;

    const hasMultiplePages = totalQueued > queuePageSize;

    if (btnFirst) btnFirst.style.display = (hasMultiplePages && currentQueuePage > 1) ? 'inline-block' : 'none';
    if (btnPrev) btnPrev.style.display = (hasMultiplePages && currentQueuePage > 1) ? 'inline-block' : 'none';
    if (btnNext) btnNext.style.display = (hasMultiplePages && currentQueuePage < totalPages) ? 'inline-block' : 'none';
    if (btnLast) btnLast.style.display = (hasMultiplePages && currentQueuePage < totalPages) ? 'inline-block' : 'none';
}

async function enqueueTrack(trackId, playNext = false) {
    try {
        const res = await fetch('/api/v1/queue/add', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ track_id: trackId, play_next: playNext })
        });
        const status = await res.json();
        updateUI(status);
    } catch (e) {
        console.error('Error enqueuing track:', e);
    }
}

async function enqueueAlbum(albumName, playNext = false) {
    try {
        const res = await fetch('/api/v1/queue/add', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ album_name: albumName, play_next: playNext })
        });
        const status = await res.json();
        updateUI(status);
    } catch (e) {
        console.error('Error enqueuing album:', e);
    }
}

async function moveQueueItem(fromIndex, toIndex) {
    try {
        const res = await fetch('/api/v1/queue/move', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ from_index: fromIndex, to_index: toIndex })
        });
        const status = await res.json();
        updateUI(status);
    } catch (e) {
        console.error('Error moving queue item:', e);
    }
}

async function removeQueueItem(index) {
    try {
        const res = await fetch(`/api/v1/queue/remove/${index}`, { method: 'DELETE' });
        const status = await res.json();
        updateUI(status);
    } catch (e) {
        console.error('Error removing queue item:', e);
    }
}

async function clearQueue() {
    try {
        const res = await fetch('/api/v1/queue/clear', { method: 'DELETE' });
        const status = await res.json();
        updateUI(status);
    } catch (e) {
        console.error('Error clearing queue:', e);
    }
}

async function skipBack() {
    try {
        const res = await fetch('/api/v1/player/previous', { method: 'POST' });
        const status = await res.json();
        updateUI(status);
    } catch (e) {
        console.error('Error skipping back:', e);
    }
}

// Track List & Album Grid Event Delegation
const libraryTbodyEl = document.getElementById('library-tbody');
if (libraryTbodyEl) {
    libraryTbodyEl.addEventListener('click', (e) => {
        const playNowBtn = e.target.closest('.btn-play-now');
        const playNextBtn = e.target.closest('.btn-enqueue-next');
        const queueAddBtn = e.target.closest('.btn-enqueue-add');
        const row = e.target.closest('.track-row');

        if (playNowBtn) {
            e.stopPropagation();
            playTrack(playNowBtn.getAttribute('data-id'));
        } else if (playNextBtn) {
            e.stopPropagation();
            enqueueTrack(playNextBtn.getAttribute('data-id'), true);
        } else if (queueAddBtn) {
            e.stopPropagation();
            enqueueTrack(queueAddBtn.getAttribute('data-id'), false);
        } else if (row) {
            playTrack(row.getAttribute('data-id'));
        }
    });
}

const albumsGridEl = document.getElementById('albums-grid');
if (albumsGridEl) {
    albumsGridEl.addEventListener('click', (e) => {
        const nextBtn = e.target.closest('.btn-album-next');
        const addBtn = e.target.closest('.btn-album-add');
        const card = e.target.closest('.album-card');

        if (nextBtn) {
            e.stopPropagation();
            enqueueAlbum(nextBtn.getAttribute('data-album'), true);
        } else if (addBtn) {
            e.stopPropagation();
            enqueueAlbum(addBtn.getAttribute('data-album'), false);
        } else if (card) {
            openAlbumTracklist(card.getAttribute('data-album'));
        }
    });
}

async function fetchStatus() {
    try {
        const res = await fetch('/api/v1/status');
        if (res.ok) {
            const status = await res.json();
            updateUI(status);
        }
    } catch (e) {
        console.error('Error polling status:', e);
    }
}

// Queue Pagination Listeners
const queuePageSizeSelect = document.getElementById('queue-page-size-select');
if (queuePageSizeSelect) {
    queuePageSizeSelect.addEventListener('change', (e) => {
        queuePageSize = parseInt(e.target.value);
        currentQueuePage = 1;
        if (currentStatus) renderQueue(currentStatus.current_track, currentStatus.queue || []);
    });
}

const btnFirstQueue = document.getElementById('btn-first-queue');
const btnPrevQueue = document.getElementById('btn-prev-queue');
const btnNextQueue = document.getElementById('btn-next-queue');
const btnLastQueue = document.getElementById('btn-last-queue');

if (btnFirstQueue) btnFirstQueue.addEventListener('click', () => { currentQueuePage = 1; if (currentStatus) renderQueue(currentStatus.current_track, currentStatus.queue || []); });
if (btnPrevQueue) btnPrevQueue.addEventListener('click', () => { if (currentQueuePage > 1) { currentQueuePage--; if (currentStatus) renderQueue(currentStatus.current_track, currentStatus.queue || []); } });
if (btnNextQueue) btnNextQueue.addEventListener('click', () => { currentQueuePage++; if (currentStatus) renderQueue(currentStatus.current_track, currentStatus.queue || []); });
if (btnLastQueue) btnLastQueue.addEventListener('click', () => { if (currentStatus && currentStatus.queue) { currentQueuePage = Math.ceil(currentStatus.queue.length / queuePageSize); renderQueue(currentStatus.current_track, currentStatus.queue); } });

// Queue Drawer Event Listeners
const btnPrevTrack = document.getElementById('btn-prev');
if (btnPrevTrack) {
    btnPrevTrack.addEventListener('click', skipBack);
}

const btnToggleQueue = document.getElementById('btn-toggle-queue');
const btnCloseQueue = document.getElementById('btn-close-queue');
const queueOverlay = document.getElementById('queue-drawer-overlay');
const btnClearQueue = document.getElementById('btn-clear-queue');

if (btnToggleQueue) btnToggleQueue.addEventListener('click', () => toggleQueueDrawer());
if (btnCloseQueue) btnCloseQueue.addEventListener('click', () => toggleQueueDrawer(false));
if (queueOverlay) queueOverlay.addEventListener('click', () => toggleQueueDrawer(false));
if (btnClearQueue) btnClearQueue.addEventListener('click', () => { clearQueue(); currentQueuePage = 1; });

// Initialize on Load
window.addEventListener('DOMContentLoaded', () => {
    initWebSocket();
    fetchLibrary();
    startClock();
    fetchStatus();
    setInterval(fetchStatus, 2000); // Periodic fallback polling every 2s to guarantee 100% sync
});
