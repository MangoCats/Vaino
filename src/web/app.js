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

// Fetch & Render Library Tracks
async function fetchLibrary(query = '') {
    try {
        const url = query ? `/api/v1/library/tracks?query=${encodeURIComponent(query)}` : '/api/v1/library/tracks';
        const res = await fetch(url);
        const data = await res.json();
        tracksList = data.tracks;
        renderLibrary(tracksList);
    } catch (e) {
        console.error('Error fetching library:', e);
        libraryTbody.innerHTML = `<tr><td colspan="6" class="loading-cell">Failed loading library tracks.</td></tr>`;
    }
}

function renderLibrary(tracks) {
    if (!tracks || tracks.length === 0) {
        libraryTbody.innerHTML = `<tr><td colspan="6" class="loading-cell">No tracks found in library.</td></tr>`;
        return;
    }

    libraryTbody.innerHTML = tracks.map((t, idx) => `
        <tr onclick="playTrack('${t.id}')">
            <td>${idx + 1}</td>
            <td><strong>${escapeHtml(t.title)}</strong></td>
            <td>${escapeHtml(t.artist)}</td>
            <td>${escapeHtml(t.album || '-')}</td>
            <td>${formatTime(t.duration_ms)}</td>
            <td><button class="btn-play-track" onclick="event.stopPropagation(); playTrack('${t.id}')">▶ Play</button></td>
        </tr>
    `).join('');
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

librarySearch.addEventListener('input', (e) => {
    fetchLibrary(e.target.value);
});

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
