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
}

// Library Pagination & View State
let currentView = 'tracks'; // 'tracks', 'artists', 'albums'
let currentPage = 1;
let pageSize = 100;
let totalTracks = 0;
let currentQuery = '';
let currentLetter = '';
let currentArtistFilter = '';
let currentAlbumFilter = '';

// Fetch & Render Library Tracks
async function fetchLibrary(page = 1) {
    if (currentView === 'artists' && !currentArtistFilter && !currentAlbumFilter) return fetchArtists();
    if (currentView === 'albums' && !currentArtistFilter && !currentAlbumFilter) return fetchAlbums();

    currentPage = page;
    const offset = (currentPage - 1) * pageSize;

    try {
        let url = `/api/v1/library/tracks?limit=${pageSize}&offset=${offset}`;
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

// Fetch & Render Artists Grid [REQ-UI-020A, REQ-UI-020E]
async function fetchArtists() {
    const grid = document.getElementById('artists-grid');
    if (!grid) return;
    grid.innerHTML = `<div class="loading-cell">Loading artists...</div>`;

    try {
        let url = `/api/v1/library/artists?limit=200`;
        if (currentLetter) {
            url += `&letter=${encodeURIComponent(currentLetter)}`;
        }
        if (currentQuery) {
            url += `&query=${encodeURIComponent(currentQuery)}`;
        }
        const res = await fetch(url);
        const data = await res.json();
        const artists = data.artists || [];

        const countBadge = document.getElementById('library-count-badge');
        if (countBadge) countBadge.textContent = `${artists.length} artists`;

        if (artists.length === 0) {
            grid.innerHTML = `<div class="loading-cell">No artists found.</div>`;
            return;
        }

        grid.innerHTML = artists.map(a => `
            <div class="nav-card" onclick="browseArtistAlbums('${escapeHtml(a.artist)}')">
                <img src="/api/v1/art/${a.sample_track_id}" class="nav-card-art" onerror="this.src='data:image/svg+xml;utf8,<svg xmlns=\\'http://www.w3.org/2000/svg\\' width=\\'140\\' height=\\'140\\'><rect width=\\'140\\' height=\\'140\\' fill=\\'%231e2230\\'/><text x=\\'50%\\' y=\\'50%\\' dominant-baseline=\\'middle\\' text-anchor=\\'middle\\' fill=\\'%234a5568\\' font-size=\\'36\\'>🎙️</text></svg>'">
                <div class="nav-card-title">${escapeHtml(a.artist)}</div>
                <div class="nav-card-subtitle">${a.album_count} Albums • ${a.track_count} Tracks</div>
                <span class="nav-card-badge">View Albums ▶</span>
            </div>
        `).join('');
    } catch (e) {
        console.error('Error fetching artists:', e);
        grid.innerHTML = `<div class="loading-cell">Failed loading artists.</div>`;
    }
}

// Fetch & Render Albums Grid [REQ-UI-020A, REQ-UI-020D, REQ-UI-020E]
async function fetchAlbums(artistFilter = null) {
    const grid = document.getElementById('albums-grid');
    if (!grid) return;
    grid.innerHTML = `<div class="loading-cell">Loading albums...</div>`;

    const activeArtist = artistFilter || currentArtistFilter;

    try {
        let url = `/api/v1/library/albums?limit=200`;
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
        renderAlbumsGrid(data.albums || [], activeArtist);
    } catch (e) {
        console.error('Error fetching albums:', e);
        grid.innerHTML = `<div class="loading-cell">Failed loading albums.</div>`;
    }
}

// Drill-down from Artist card to their Albums [REQ-UI-020C, REQ-UI-020D]
async function browseArtistAlbums(artistName) {
    currentArtistFilter = artistName;
    currentLetter = '';
    document.querySelectorAll('.letter-btn').forEach(b => b.classList.remove('active'));
    const allLetterBtn = document.querySelector('.letter-btn[data-letter=""]');
    if (allLetterBtn) allLetterBtn.classList.add('active');

    try {
        const url = `/api/v1/library/albums?artist=${encodeURIComponent(artistName)}`;
        const res = await fetch(url);
        const data = await res.json();
        const albums = data.albums || [];

        if (albums.length === 1) {
            // Single album: direct navigation to that album's sorted tracklist!
            openAlbumTracklist(albums[0].album, artistName);
        } else {
            // Multiple albums: show subset album selection screen containing ONLY that artist's albums
            switchView('albums');
            renderAlbumsGrid(albums, artistName);
            setBreadcrumbFilter('Artist', artistName);
        }
    } catch (e) {
        console.error('Error browsing artist albums:', e);
    }
}

function renderAlbumsGrid(albums, artistFilter = null) {
    const grid = document.getElementById('albums-grid');
    if (!grid) return;

    const countBadge = document.getElementById('library-count-badge');
    if (countBadge) {
        countBadge.textContent = artistFilter ? `${artistFilter} (${albums.length} albums)` : `${albums.length} albums`;
    }

    if (albums.length === 0) {
        grid.innerHTML = `<div class="loading-cell">No albums found.</div>`;
        return;
    }

    grid.innerHTML = albums.map(al => `
        <div class="nav-card" onclick="openAlbumTracklist('${escapeHtml(al.album)}', '${escapeHtml(al.artist)}')">
            <img src="/api/v1/art/${al.sample_track_id}" class="nav-card-art" onerror="this.src='data:image/svg+xml;utf8,<svg xmlns=\\'http://www.w3.org/2000/svg\\' width=\\'140\\' height=\\'140\\'><rect width=\\'140\\' height=\\'140\\' fill=\\'%231e2230\\'/><text x=\\'50%\\' y=\\'50%\\' dominant-baseline=\\'middle\\' text-anchor=\\'middle\\' fill=\\'%234a5568\\' font-size=\\'36\\'>💿</text></svg>'">
            <div class="nav-card-title">${escapeHtml(al.album)}</div>
            <div class="nav-card-subtitle">${escapeHtml(al.artist)} ${al.year ? '(' + al.year + ')' : ''}</div>
            <span class="nav-card-badge">${al.track_count} Tracks ▶</span>
        </div>
    `).join('');
}

// Drill-down from Album card to sorted Tracklist [REQ-UI-020B, REQ-UI-020D, REQ-UI-020F]
async function openAlbumTracklist(albumName, artistName) {
    currentAlbumFilter = albumName;
    if (artistName) currentArtistFilter = artistName;
    
    currentLetter = '';
    document.querySelectorAll('.letter-btn').forEach(b => b.classList.remove('active'));
    const allLetterBtn = document.querySelector('.letter-btn[data-letter=""]');
    if (allLetterBtn) allLetterBtn.classList.add('active');

    switchView('tracks');
    try {
        let url = `/api/v1/library/albums/${encodeURIComponent(albumName)}/tracks`;
        if (artistName) url += `?artist=${encodeURIComponent(artistName)}`;
        const res = await fetch(url);
        const data = await res.json();
        
        tracksList = data.tracks || [];
        totalTracks = tracksList.length;
        renderLibrary(tracksList, 0);
        updatePaginationControls();

        const countBadge = document.getElementById('library-count-badge');
        if (countBadge) countBadge.textContent = `${albumName} (${totalTracks} tracks)`;

        setBreadcrumbFilter('Album', artistName ? `${albumName} (${artistName})` : albumName);
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
        <tr onclick="playTrack('${t.id}')">
            <td>${offset + idx + 1}</td>
            <td><strong>${escapeHtml(t.title)}</strong></td>
            <td>${escapeHtml(t.artist)}</td>
            <td>${escapeHtml(t.album || '-')}</td>
            <td>${formatTime(t.duration_ms)}</td>
            <td><button class="btn-play-track" onclick="event.stopPropagation(); playTrack('${t.id}')">▶ Play</button></td>
        </tr>
    `).join('');
}

function updatePaginationControls() {
    const countBadge = document.getElementById('library-count-badge');
    const paginationInfo = document.getElementById('pagination-info');
    const pageIndicator = document.getElementById('page-indicator');
    const btnPrev = document.getElementById('btn-prev-page');
    const btnNext = document.getElementById('btn-next-page');

    const totalPages = Math.max(1, Math.ceil(totalTracks / pageSize));
    const startItem = totalTracks === 0 ? 0 : (currentPage - 1) * pageSize + 1;
    const endItem = Math.min(totalTracks, currentPage * pageSize);

    if (countBadge) countBadge.textContent = `${totalTracks.toLocaleString()} tracks`;
    if (paginationInfo) paginationInfo.textContent = `Showing ${startItem.toLocaleString()}–${endItem.toLocaleString()} of ${totalTracks.toLocaleString()} tracks`;
    if (pageIndicator) pageIndicator.textContent = `Page ${currentPage} of ${totalPages}`;

    if (btnPrev) btnPrev.disabled = (currentPage <= 1);
    if (btnNext) btnNext.disabled = (currentPage >= totalPages);
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
    await fetch('/api/v1/player/skip', { method: 'POST' });
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

// Pagination Button Listeners
const btnPrev = document.getElementById('btn-prev-page');
const btnNext = document.getElementById('btn-next-page');
const pageSizeSelect = document.getElementById('page-size-select');

if (btnPrev) {
    btnPrev.addEventListener('click', () => {
        if (currentPage > 1) fetchLibrary(currentPage - 1);
    });
}

if (btnNext) {
    btnNext.addEventListener('click', () => {
        fetchLibrary(currentPage + 1);
    });
}

if (pageSizeSelect) {
    pageSizeSelect.addEventListener('change', (e) => {
        pageSize = parseInt(e.target.value);
        fetchLibrary(1);
    });
}

// Breadcrumb Filter Clear Listener [REQ-UI-020D]
const btnClearBreadcrumb = document.getElementById('btn-clear-breadcrumb');
if (btnClearBreadcrumb) {
    btnClearBreadcrumb.addEventListener('click', () => {
        clearBreadcrumbFilter();
    });
}

// View Tabs Navigation Listener [REQ-UI-020A]
const viewTabs = document.getElementById('view-tabs');
if (viewTabs) {
    viewTabs.addEventListener('click', (e) => {
        if (e.target.classList.contains('view-tab')) {
            const targetView = e.target.getAttribute('data-view');
            if (targetView) switchView(targetView);
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

// Initialize on Load
window.addEventListener('DOMContentLoaded', () => {
    initWebSocket();
    fetchLibrary();
    startClock();
});
