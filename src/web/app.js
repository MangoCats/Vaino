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

    const letterBar = document.getElementById('letter-nav-bar');
    const searchBox = document.querySelector('.search-box');
    const breadcrumbBar = document.getElementById('filter-breadcrumb-bar');
    const countBadge = document.getElementById('library-count-badge');

    if (targetView === 'programs') {
        if (letterBar) letterBar.style.display = 'none';
        if (searchBox) searchBox.style.display = 'none';
        if (breadcrumbBar) breadcrumbBar.style.display = 'none';
        if (countBadge) countBadge.textContent = 'Time-of-Day Rotation Slots';
        loadPrograms();
    } else {
        if (letterBar) letterBar.style.display = 'flex';
        if (searchBox) searchBox.style.display = 'block';
        if (targetView === 'tracks') fetchLibrary(1);
        else if (targetView === 'artists') fetchArtists();
        else if (targetView === 'albums') fetchAlbums(currentArtistFilter);
    }
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
                    <button class="btn-action-sm btn-descriptors" data-id="${t.id}" title="View Acoustic Descriptors">🧠</button>
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
            <div class="queue-item-card" data-queue-idx="${actualIdx}" data-title="${escapeHtml(item.title)}">
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
                    <button class="btn-queue-action" onclick="openDescriptorsModal('${item.id}')" title="View Acoustic Descriptors">🧠</button>
                    <button class="btn-queue-action btn-queue-delete" onclick="removeQueueItem(${actualIdx}, event)" title="Remove from Queue">🗑</button>
                </div>
            </div>
        `;
    }).join('');

    updateQueuePagination(totalQueued);
}

function showToast(message) {
    let container = document.getElementById('toast-container');
    if (!container) {
        container = document.createElement('div');
        container.id = 'toast-container';
        document.body.appendChild(container);
    }

    const toast = document.createElement('div');
    toast.className = 'toast-notification';
    toast.innerHTML = `<span>${escapeHtml(message)}</span>`;
    container.appendChild(toast);

    requestAnimationFrame(() => {
        toast.classList.add('toast-visible');
    });

    setTimeout(() => {
        toast.classList.remove('toast-visible');
        setTimeout(() => {
            if (toast.parentNode) toast.parentNode.removeChild(toast);
        }, 350);
    }, 3000);
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
        showToast(playNext ? '➕ Track added to play next' : '📥 Track added to queue');
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
        showToast(playNext ? `➕ Album "${albumName}" added next` : `📥 Album "${albumName}" added to queue`);
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

async function removeQueueItem(index, event) {
    let cardEl = null;
    if (event && event.target) {
        cardEl = event.target.closest('.queue-item-card');
    }
    if (!cardEl) {
        cardEl = document.querySelector(`.queue-item-card[data-queue-idx="${index}"]`);
    }

    let trackTitle = '';
    if (cardEl) {
        trackTitle = cardEl.getAttribute('data-title') || '';
        cardEl.classList.add('queue-item-removing');
    }

    if (trackTitle) {
        showToast(`🗑 Removed "${trackTitle}" from queue`);
    } else {
        showToast(`🗑 Track removed from queue`);
    }

    try {
        const res = await fetch(`/api/v1/queue/remove/${index}`, { method: 'DELETE' });
        const status = await res.json();
        updateUI(status);
    } catch (e) {
        console.error('Error removing queue item:', e);
        if (cardEl) cardEl.classList.remove('queue-item-removing');
    }
}

async function clearQueue() {
    try {
        const res = await fetch('/api/v1/queue/clear', { method: 'DELETE' });
        const status = await res.json();
        showToast('🗑 Queue cleared');
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
        const descriptorsBtn = e.target.closest('.btn-descriptors');
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
        } else if (descriptorsBtn) {
            e.stopPropagation();
            openDescriptorsModal(descriptorsBtn.getAttribute('data-id'));
        } else if (row) {
            playTrack(row.getAttribute('data-id'));
        }
    });
}

async function openDescriptorsModal(trackId) {
    const modalOverlay = document.getElementById('descriptors-modal-overlay');
    const modalTitle = document.getElementById('descriptors-modal-title');
    const modalSubtitle = document.getElementById('descriptors-modal-subtitle');
    const modalBody = document.getElementById('descriptors-modal-body');

    if (!modalOverlay || !modalBody) return;

    modalOverlay.style.display = 'flex';
    modalBody.innerHTML = `<div class="loading-cell">Loading acoustic descriptors...</div>`;

    try {
        const resp = await fetch(`/api/v1/descriptors/${trackId}`);
        if (!resp.ok) throw new Error('Failed to fetch descriptors');
        const data = await resp.json();
        const t = data.track || {};
        const d = data.descriptors || {};
        const isFallback = data.is_fallback || false;
        modalTitle.textContent = `🧠 ${t.title || 'Acoustic Descriptors'}`;
        modalSubtitle.textContent = `${t.artist || 'Unknown Artist'} — ${t.album || 'Unknown Album'}${isFallback ? ' (⚠️ 0-byte File / Unanalyzed Fallback)' : ''}`;

        const rows = [
            {
                name: '⚡ Energy',
                val: (d.energy ?? 0.5).toFixed(3),
                concept: (d.energy ?? 0.5) >= 0.5 ? 'High Intensity / Peak' : 'Mellow / Low Intensity',
                pct: Math.round((d.energy ?? 0.5) * 100)
            },
            {
                name: '😊 Valence',
                val: (d.valence ?? 0.5).toFixed(3),
                concept: (d.valence ?? 0.5) >= 0.5 ? 'Happy / Positive Mood' : 'Melancholic / Sad',
                pct: Math.round((d.valence ?? 0.5) * 100)
            },
            {
                name: '💃 Danceability',
                val: (d.danceability ?? 0.5).toFixed(3),
                concept: (d.danceability ?? 0.5) >= 0.5 ? 'Danceable (Rhythmic)' : 'Not Danceable',
                pct: Math.round((d.danceability ?? 0.5) * 100)
            },
            {
                name: '🎻 Acousticness',
                val: (d.acousticness ?? 0.5).toFixed(3),
                concept: (d.acousticness ?? 0.5) >= 0.5 ? 'Acoustic' : 'Electronic / Electric',
                pct: Math.round((d.acousticness ?? 0.5) * 100)
            },
            {
                name: '🎺 Instrumentalness',
                val: (d.instrumentalness ?? 0.5).toFixed(3),
                concept: (d.instrumentalness ?? 0.5) >= 0.5 ? 'Instrumental Track' : 'Vocal / Lyric-focused',
                pct: Math.round((d.instrumentalness ?? 0.5) * 100)
            },
            {
                name: '🗣️ Speechiness',
                val: (d.speechiness ?? 0.1).toFixed(3),
                concept: (d.speechiness ?? 0.1) >= 0.33 ? 'Spoken Word / Voice' : 'Music / Song',
                pct: Math.round((d.speechiness ?? 0.1) * 100)
            },
            {
                name: '🥁 Tempo',
                val: `${(d.tempo_bpm ?? 120).toFixed(1)} BPM`,
                concept: (d.tempo_bpm ?? 120) < 90 ? 'Slow (Lento)' : ((d.tempo_bpm ?? 120) <= 130 ? 'Moderate (Andante)' : 'Fast (Presto)'),
                pct: Math.round(Math.max(0, Math.min(100, ((d.tempo_bpm ?? 120) - 60) / 140 * 100)))
            },
            {
                name: '🎹 Key Signature',
                val: d.key_signature || 'C Major',
                concept: 'Tonal Key Center',
                pct: null
            },
            {
                name: '🔊 Loudness',
                val: `${(d.loudness_lufs ?? -14).toFixed(2)} LUFS`,
                concept: 'EBU R128 Integrated Target Leveling',
                pct: Math.round(Math.max(0, Math.min(100, ((d.loudness_lufs ?? -14) + 60) / 60 * 100)))
            }
        ];

        const esData = d.essentia || {};
        const gender = esData.gender || {};
        const timbre = esData.timbre || {};
        const moodAgg = esData.mood_aggressive || {};
        const moodParty = esData.mood_party || {};
        const moodRelaxed = esData.mood_relaxed || {};
        const moodSad = esData.mood_sad || {};
        const genreRos = esData.genre_rosamerica || {};

        if (gender.female !== undefined) {
            const fVal = gender.female || 0.5;
            const mVal = gender.male || (1.0 - fVal);
            let genderText = 'Dual Vocal / Instrumental (50.0%)';
            let genderConcept = 'Neutral Pitch Harmonic Balance';
            if (fVal > 0.55) {
                genderText = `Female (${(fVal * 100).toFixed(1)}%)`;
                genderConcept = 'Female Lead Vocalist Pitch F0 Model';
            } else if (mVal > 0.55) {
                genderText = `Male (${(mVal * 100).toFixed(1)}%)`;
                genderConcept = 'Male Lead Vocalist Pitch F0 Model';
            }
            rows.push({
                name: '🎙️ Vocal Gender',
                val: genderText,
                concept: genderConcept,
                pct: Math.round(fVal * 100)
            });
        }
        if (timbre.bright !== undefined) {
            rows.push({
                name: '🎸 Timbre Profile',
                val: (timbre.bright >= 0.5) ? `Bright (${(timbre.bright * 100).toFixed(1)}%)` : `Dark (${((timbre.dark || 0.5) * 100).toFixed(1)}%)`,
                concept: 'Spectral Centroid Timbre Balance',
                pct: Math.round((timbre.bright || 0.5) * 100)
            });
        }
        if (moodAgg.aggressive !== undefined) {
            rows.push({
                name: '💥 Mood Aggressive',
                val: (moodAgg.aggressive >= 0.5) ? `Aggressive (${(moodAgg.aggressive * 100).toFixed(1)}%)` : `Not Aggressive (${((moodAgg.not_aggressive || 0.5) * 100).toFixed(1)}%)`,
                concept: 'Zero Crossing Dynamics Model',
                pct: Math.round((moodAgg.aggressive || 0.1) * 100)
            });
        }
        if (moodParty.party !== undefined) {
            rows.push({
                name: '🎉 Mood Party',
                val: (moodParty.party >= 0.5) ? `Party (${(moodParty.party * 100).toFixed(1)}%)` : `Mellow (${((moodParty.not_party || 0.5) * 100).toFixed(1)}%)`,
                concept: 'Rhythmic Beat Density Model',
                pct: Math.round((moodParty.party || 0.5) * 100)
            });
        }
        if (genreRos.cla !== undefined) {
            const topGenre = Object.entries(genreRos).sort((a, b) => b[1] - a[1])[0];
            const genreNames = { cla: 'Classical', dan: 'Dance', hip: 'Hip-Hop', jaz: 'Jazz', pop: 'Pop', rhy: 'R&B/Soul', roc: 'Rock', spe: 'Speech' };
            rows.push({
                name: '📻 Primary Genre (Rosamerica)',
                val: `${genreNames[topGenre[0]] || topGenre[0]} (${(topGenre[1] * 100).toFixed(1)}%)`,
                concept: 'Multi-class Rosamerica ML Profile',
                pct: Math.round((topGenre[1] || 0.1) * 100)
            });
        }

        modalBody.innerHTML = `
            <table class="descriptors-table">
                <thead>
                    <tr>
                        <th>Descriptor</th>
                        <th>Value</th>
                        <th>Categorization Concept</th>
                        <th>Bar Graph</th>
                    </tr>
                </thead>
                <tbody>
                    ${rows.map(r => `
                        <tr>
                            <td><span class="descriptor-name">${r.name}</span></td>
                            <td><span class="descriptor-value-badge">${r.val}</span></td>
                            <td><span class="descriptor-concept">${r.concept}</span></td>
                            <td>
                                ${r.pct !== null ? `
                                    <div class="descriptor-bar-track">
                                        <div class="descriptor-bar-fill" style="width: ${r.pct}%;"></div>
                                    </div>
                                ` : '<span class="descriptor-concept">—</span>'}
                            </td>
                        </tr>
                    `).join('')}
                </tbody>
            </table>
        `;
    } catch (e) {
        modalBody.innerHTML = `<div class="loading-cell" style="color: var(--danger-color);">Error loading descriptors: ${escapeHtml(e.message)}</div>`;
    }
}

// Descriptors Modal Close Event Handlers
const closeDescriptorsModalBtn = document.getElementById('btn-close-descriptors-modal');
const descriptorsModalOverlay = document.getElementById('descriptors-modal-overlay');

if (closeDescriptorsModalBtn) {
    closeDescriptorsModalBtn.addEventListener('click', () => {
        if (descriptorsModalOverlay) descriptorsModalOverlay.style.display = 'none';
    });
}
if (descriptorsModalOverlay) {
    descriptorsModalOverlay.addEventListener('click', (e) => {
        if (e.target === descriptorsModalOverlay) {
            descriptorsModalOverlay.style.display = 'none';
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

// Keyboard Shortcuts Listener [REQ-UI-010C]
document.addEventListener('keydown', (e) => {
    if (['INPUT', 'TEXTAREA', 'SELECT'].includes(document.activeElement.tagName)) return;
    if (e.code === 'Space') {
        e.preventDefault();
        togglePlay();
    } else if (e.code === 'ArrowRight') {
        e.preventDefault();
        skipTrack();
    } else if (e.code === 'ArrowLeft') {
        e.preventDefault();
        skipBack();
    }
});

// --- Time-of-Day Programs & Slot Editor Management ---
let currentPrograms = [];
let editingProgramRefTrackIds = [];

async function loadPrograms() {
    const gridEl = document.getElementById('programs-grid');
    if (!gridEl) return;

    try {
        const res = await fetch('/api/v1/programs');
        if (!res.ok) throw new Error('Failed to load programs');
        const data = await res.json();

        currentPrograms = data.programs || [];
        const activeId = data.active_program_id;
        const toggleEl = document.getElementById('autoselect-toggle');
        if (toggleEl) {
            toggleEl.checked = data.use_clock_autoselect !== false;
        }

        renderProgramsTimeline(currentPrograms, activeId);
        renderProgramsGrid(currentPrograms, activeId);
    } catch (e) {
        gridEl.innerHTML = `<div class="loading-cell" style="color: var(--danger-color);">Error loading programs: ${escapeHtml(e.message)}</div>`;
    }
}

function getEffectiveTimeRanges(programs) {
    if (!programs || programs.length === 0) return {};
    const sorted = [...programs].sort((a, b) => a.start_time.localeCompare(b.start_time));
    const ranges = {};
    for (let i = 0; i < sorted.length; i++) {
        const curr = sorted[i];
        const next = sorted[(i + 1) % sorted.length];
        ranges[curr.id] = {
            start: curr.start_time,
            end: next.start_time,
            label: `${curr.start_time} – ${next.start_time}`
        };
    }
    return ranges;
}

function renderProgramsTimeline(programs, activeId) {
    const barEl = document.getElementById('programs-timeline-bar');
    if (!barEl) return;
    if (!programs || programs.length === 0) {
        barEl.innerHTML = '<div style="padding: 10px; color: var(--text-muted);">No time slot programs configured.</div>';
        return;
    }

    const sorted = [...programs].sort((a, b) => a.start_time.localeCompare(b.start_time));
    const parseMins = (t) => { const [h, m] = t.split(':').map(Number); return h * 60 + m; };
    const firstStartMins = parseMins(sorted[0].start_time);
    const lastProg = sorted[sorted.length - 1];

    let html = '';

    // 1. Midnight Wrap-Around Segment (00:00 to first start time)
    if (firstStartMins > 0) {
        const pctWidth = (firstStartMins / 1440) * 100;
        const isActive = lastProg.id === activeId;
        html += `
            <div class="timeline-segment ${isActive ? 'active' : ''}" style="width: ${pctWidth.toFixed(2)}%;" title="${escapeHtml(lastProg.name)} (00:00 - ${sorted[0].start_time})">
                <span class="segment-name">${escapeHtml(lastProg.name)}</span>
                <span class="segment-time">00:00 – ${sorted[0].start_time}</span>
            </div>
        `;
    }

    // 2. Main Segments across the day (04:00 to 24:00)
    for (let i = 0; i < sorted.length; i++) {
        const p = sorted[i];
        const startMins = parseMins(p.start_time);
        const nextStartMins = (i < sorted.length - 1) ? parseMins(sorted[i + 1].start_time) : 1440;
        const durationMins = nextStartMins - startMins;
        const pctWidth = (durationMins / 1440) * 100;
        const isActive = p.id === activeId;
        const endLabel = (i < sorted.length - 1) ? sorted[i + 1].start_time : '24:00';

        html += `
            <div class="timeline-segment ${isActive ? 'active' : ''}" style="width: ${pctWidth.toFixed(2)}%;" title="${escapeHtml(p.name)} (${p.start_time} - ${endLabel})">
                <span class="segment-name">${escapeHtml(p.name)}</span>
                <span class="segment-time">${p.start_time} – ${endLabel}</span>
            </div>
        `;
    }

    barEl.innerHTML = html;
}

function renderProgramsGrid(programs, activeId) {
    const gridEl = document.getElementById('programs-grid');
    if (!gridEl) return;
    if (!programs || programs.length === 0) {
        gridEl.innerHTML = '<div class="loading-cell">No time-slot programs configured. Click "+ Add Time Slot" or "Re-sync Defaults".</div>';
        return;
    }

    const effectiveRanges = getEffectiveTimeRanges(programs);

    gridEl.innerHTML = programs.map(p => {
        const isActive = p.id === activeId;
        const refCount = p.track_ids ? p.track_ids.split('\n').filter(Boolean).length : 0;
        const targetVec = p.target_vector || {};
        const rangeLabel = effectiveRanges[p.id] ? effectiveRanges[p.id].label : p.start_time;

        const getPct = (val) => (val !== undefined && val !== null) ? Math.round(val * 100) : 50;
        const ac = getPct(targetVec.ab_acoustic);
        const da = getPct(targetVec.ab_danceable);
        const rx = getPct(targetVec.ab_relaxed);
        const ag = getPct(targetVec.ab_aggressive);

        return `
            <div class="program-card ${isActive ? 'active-slot' : ''}" data-program-id="${p.id}">
                <div class="program-card-header">
                    <div class="program-title-group">
                        <span class="program-name">${escapeHtml(p.name)}</span>
                        <span class="program-time-badge">🕒 Effective: ${rangeLabel}</span>
                    </div>
                    ${isActive ? '<span class="active-pill">🔥 ACTIVE NOW</span>' : ''}
                </div>

                <div class="program-card-body">
                    <div class="program-metric-row">
                        <span class="metric-label">Seed Reference Tracks:</span>
                        <span class="metric-val">${refCount} seed tracks</span>
                    </div>

                    <div class="profile-bars-grid">
                        <div class="profile-bar-item">
                            <span class="pbar-label">Acoustic: <strong>${ac}%</strong></span>
                            <div class="pbar-track"><div class="pbar-fill" style="width: ${ac}%;"></div></div>
                        </div>
                        <div class="profile-bar-item">
                            <span class="pbar-label">Danceable: <strong>${da}%</strong></span>
                            <div class="pbar-track"><div class="pbar-fill" style="width: ${da}%;"></div></div>
                        </div>
                        <div class="profile-bar-item">
                            <span class="pbar-label">Relaxed: <strong>${rx}%</strong></span>
                            <div class="pbar-track"><div class="pbar-fill" style="width: ${rx}%;"></div></div>
                        </div>
                        <div class="profile-bar-item">
                            <span class="pbar-label">Aggressive: <strong>${ag}%</strong></span>
                            <div class="pbar-track"><div class="pbar-fill" style="width: ${ag}%;"></div></div>
                        </div>
                    </div>
                </div>

                <div class="program-card-actions">
                    <button class="btn-control btn-secondary btn-sm btn-edit-program" data-id="${p.id}">✏️ Edit Slot</button>
                    <button class="btn-control btn-danger btn-sm btn-delete-program" data-id="${p.id}">🗑 Delete</button>
                </div>
            </div>
        `;
    }).join('');
}

// Program Event Listeners & Modals
const programsGridEl = document.getElementById('programs-grid');
if (programsGridEl) {
    programsGridEl.addEventListener('click', (e) => {
        const editBtn = e.target.closest('.btn-edit-program');
        const delBtn = e.target.closest('.btn-delete-program');
        if (editBtn) {
            const pid = parseInt(editBtn.getAttribute('data-id'));
            openProgramModal(pid);
        } else if (delBtn) {
            const pid = parseInt(delBtn.getAttribute('data-id'));
            deleteProgramSlot(pid);
        }
    });
}

const btnAddProgram = document.getElementById('btn-add-program');
if (btnAddProgram) btnAddProgram.addEventListener('click', () => openProgramModal(null));

const btnImportMulibProgs = document.getElementById('btn-import-mulib-progs');
if (btnImportMulibProgs) {
    btnImportMulibProgs.addEventListener('click', async () => {
        try {
            const res = await fetch('/api/v1/programs/import-mulib', { method: 'POST' });
            if (res.ok) {
                loadPrograms();
            }
        } catch (e) {
            console.error('Error importing mulib programs:', e);
        }
    });
}

const autoselectToggle = document.getElementById('autoselect-toggle');
if (autoselectToggle) {
    autoselectToggle.addEventListener('change', async (e) => {
        try {
            await fetch('/api/v1/programs/toggle-autoselect', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ enabled: e.target.checked })
            });
        } catch (err) {
            console.error('Error toggling autoselect:', err);
        }
    });
}

// Program Editor Modal Functions
const programModalOverlay = document.getElementById('program-modal-overlay');
const btnCloseProgramModal = document.getElementById('btn-close-program-modal');
const btnCancelProgram = document.getElementById('btn-cancel-program');
const programForm = document.getElementById('program-form');
let editingProgramRefTracks = []; // List of track objects [{id, title, artist, album}, ...]

function openProgramModal(programId = null) {
    const titleEl = document.getElementById('program-modal-title');
    const idInput = document.getElementById('program-id-input');
    const nameInput = document.getElementById('program-name-input');
    const timeInput = document.getElementById('program-time-input');

    if (programId) {
        const p = currentPrograms.find(prog => prog.id === programId);
        if (p) {
            titleEl.textContent = '🕒 Edit Time Slot Program';
            idInput.value = p.id;
            nameInput.value = p.name;
            timeInput.value = p.start_time;
            editingProgramRefTracks = p.reference_tracks ? [...p.reference_tracks] : [];
        }
    } else {
        titleEl.textContent = '🕒 Create New Time Slot Program';
        idInput.value = '';
        nameInput.value = '';
        timeInput.value = '12:00';
        editingProgramRefTracks = [];
    }

    renderProgramRefTracks();
    if (programModalOverlay) programModalOverlay.style.display = 'flex';
}

function renderProgramRefTracks() {
    const listEl = document.getElementById('program-ref-tracks-list');
    if (!listEl) return;

    if (editingProgramRefTracks.length === 0) {
        listEl.innerHTML = '<div class="empty-notice">No reference seed tracks added yet. Click "+ Add Track" below to pick reference songs.</div>';
        return;
    }

    listEl.innerHTML = editingProgramRefTracks.map(t => `
        <div class="ref-track-chip" data-id="${t.id}">
            <div class="ref-track-details">
                <span class="ref-track-title">🎵 ${escapeHtml(t.title)}</span>
                <span class="ref-track-artist">by ${escapeHtml(t.artist)}${t.album ? ` • ${escapeHtml(t.album)}` : ''}</span>
            </div>
            <button type="button" class="btn-remove-ref" data-id="${t.id}" title="Remove seed track">✕</button>
        </div>
    `).join('');
}

function closeProgramModal() {
    if (programModalOverlay) programModalOverlay.style.display = 'none';
}

if (btnCloseProgramModal) btnCloseProgramModal.addEventListener('click', closeProgramModal);
if (btnCancelProgram) btnCancelProgram.addEventListener('click', closeProgramModal);

if (programForm) {
    programForm.addEventListener('submit', async (e) => {
        e.preventDefault();
        const idVal = document.getElementById('program-id-input').value;
        const nameVal = document.getElementById('program-name-input').value.trim();
        const timeVal = document.getElementById('program-time-input').value;
        const trackIdsVal = editingProgramRefTracks.map(t => t.id).join('\n');

        try {
            const url = idVal ? `/api/v1/programs/${idVal}` : '/api/v1/programs';
            const method = idVal ? 'PUT' : 'POST';
            const res = await fetch(url, {
                method,
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ name: nameVal, start_time: timeVal, track_ids: trackIdsVal })
            });

            if (res.ok) {
                closeProgramModal();
                loadPrograms();
            }
        } catch (e) {
            console.error('Error saving program:', e);
        }
    });
}

async function deleteProgramSlot(programId) {
    if (!confirm('Are you sure you want to delete this time slot program?')) return;
    try {
        const res = await fetch(`/api/v1/programs/${programId}`, { method: 'DELETE' });
        if (res.ok) {
            loadPrograms();
        }
    } catch (e) {
        console.error('Error deleting program:', e);
    }
}

// Track Picker Modal Functions
const pickerModalOverlay = document.getElementById('picker-modal-overlay');
const btnClosePickerModal = document.getElementById('btn-close-picker-modal');
const btnOpenTrackPicker = document.getElementById('btn-open-track-picker');
const pickerSearchInput = document.getElementById('picker-search-input');
const pickerResultsList = document.getElementById('picker-results-list');

if (btnOpenTrackPicker) {
    btnOpenTrackPicker.addEventListener('click', () => {
        if (pickerModalOverlay) pickerModalOverlay.style.display = 'flex';
        if (pickerSearchInput) {
            pickerSearchInput.value = '';
            pickerSearchInput.focus();
            searchPickerTracks('');
        }
    });
}

if (btnClosePickerModal) {
    btnClosePickerModal.addEventListener('click', () => {
        if (pickerModalOverlay) pickerModalOverlay.style.display = 'none';
    });
}

let pickerTimeout;
if (pickerSearchInput) {
    pickerSearchInput.addEventListener('input', (e) => {
        clearTimeout(pickerTimeout);
        pickerTimeout = setTimeout(() => searchPickerTracks(e.target.value), 250);
    });
}

async function searchPickerTracks(query) {
    if (!pickerResultsList) return;
    try {
        const res = await fetch(`/api/v1/library/tracks?limit=50&query=${encodeURIComponent(query)}`);
        const data = await res.json();
        const tracks = data.tracks || [];

        if (tracks.length === 0) {
            pickerResultsList.innerHTML = '<div class="loading-cell">No matching tracks found in library.</div>';
            return;
        }

        const selectedIds = editingProgramRefTracks.map(t => t.id);

        pickerResultsList.innerHTML = `
            <table class="picker-table">
                <thead>
                    <tr>
                        <th>Track Title</th>
                        <th>Artist(s)</th>
                        <th>Album</th>
                        <th>Duration</th>
                        <th>Action</th>
                    </tr>
                </thead>
                <tbody>
                    ${tracks.map(t => {
                        const isSelected = selectedIds.includes(t.id);
                        return `
                            <tr class="picker-row ${isSelected ? 'selected' : ''}">
                                <td><strong class="picker-track-title">${escapeHtml(t.title)}</strong></td>
                                <td><span class="picker-track-artist">${escapeHtml(t.artist)}</span></td>
                                <td><span class="picker-track-album">${escapeHtml(t.album || '-')}</span></td>
                                <td>${formatTime(t.duration_ms)}</td>
                                <td>
                                    <button type="button" class="btn-action-sm ${isSelected ? 'btn-selected' : 'btn-select-seed'}" data-id="${t.id}" data-title="${escapeHtml(t.title)}" data-artist="${escapeHtml(t.artist)}" data-album="${escapeHtml(t.album || '')}">
                                        ${isSelected ? '✓ Added' : '+ Add to Slot'}
                                    </button>
                                </td>
                            </tr>
                        `;
                    }).join('')}
                </tbody>
            </table>
        `;
    } catch (e) {
        pickerResultsList.innerHTML = `<div class="loading-cell" style="color: var(--danger-color);">Error searching tracks: ${escapeHtml(e.message)}</div>`;
    }
}

if (pickerResultsList) {
    pickerResultsList.addEventListener('click', (e) => {
        const addBtn = e.target.closest('.btn-select-seed');
        if (addBtn) {
            const tid = addBtn.getAttribute('data-id');
            const title = addBtn.getAttribute('data-title');
            const artist = addBtn.getAttribute('data-artist');
            const album = addBtn.getAttribute('data-album');

            if (!editingProgramRefTracks.some(t => t.id === tid)) {
                editingProgramRefTracks.push({ id: tid, title, artist, album });
                renderProgramRefTracks();
                addBtn.textContent = '✓ Added';
                addBtn.classList.remove('btn-select-seed');
                addBtn.classList.add('btn-selected');
            }
        }
    });
}

const refTracksListEl = document.getElementById('program-ref-tracks-list');
if (refTracksListEl) {
    refTracksListEl.addEventListener('click', (e) => {
        const removeBtn = e.target.closest('.btn-remove-ref');
        if (removeBtn) {
            const tid = removeBtn.getAttribute('data-id');
            editingProgramRefTracks = editingProgramRefTracks.filter(t => t.id !== tid);
            renderProgramRefTracks();
        }
    });
}

// Initialize on Load
window.addEventListener('DOMContentLoaded', () => {
    initWebSocket();
    fetchLibrary();
    startClock();
    fetchStatus();
    setInterval(fetchStatus, 2000); // Periodic fallback polling every 2s to guarantee 100% sync
});

