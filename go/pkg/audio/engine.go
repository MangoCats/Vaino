package audio

import (
	"sync"
	"time"

	"github.com/mangocats/vaino/pkg/db"
	"github.com/mangocats/vaino/pkg/selector"
)

type PlaybackState string

const (
	StateIdle    PlaybackState = "IDLE"
	StatePlaying PlaybackState = "PLAYING"
	StatePaused  PlaybackState = "PAUSED"
)

type AudioEngine struct {
	mu              sync.Mutex
	database        *db.Database
	programDirector *selector.ProgramDirector
	state           PlaybackState
	volume          int
	currentTrack    *db.Track
	queue           []db.Track
	player          *pcmPlayer
}

func NewAudioEngine(database *db.Database) *AudioEngine {
	return &AudioEngine{
		database:        database,
		programDirector: selector.NewProgramDirector(database),
		state:           StateIdle,
		volume:          80,
		queue:           make([]db.Track, 0),
		player:          newPCMPlayer(),
	}
}

func (e *AudioEngine) ReplenishQueueIfNeeded() {
	e.mu.Lock()
	defer e.mu.Unlock()

	if e.database == nil || e.programDirector == nil {
		return
	}

	for len(e.queue) < 3 {
		existingIDs := make(map[string]bool)
		if e.currentTrack != nil {
			existingIDs[e.currentTrack.ID] = true
		}
		for _, qItem := range e.queue {
			existingIDs[qItem.ID] = true
		}

		nextTrack, err := e.programDirector.SelectNextTrack(e.currentTrack, nil, nil, existingIDs)
		if err != nil || nextTrack == nil || existingIDs[nextTrack.ID] {
			break
		}
		e.queue = append(e.queue, *nextTrack)
	}
}

func (e *AudioEngine) GetStatus() map[string]interface{} {
	e.ReplenishQueueIfNeeded()

	e.mu.Lock()
	defer e.mu.Unlock()

	var curTrackMap interface{} = nil
	var durationMs int64 = 0
	var elapsedMs int64 = 0

	if e.currentTrack != nil {
		curTrackMap = e.currentTrack
		durationMs = e.currentTrack.DurationMs
		if e.player != nil && e.state == StatePlaying {
			elapsedMs = e.player.getPositionMs()
		}
	}

	queueList := make([]db.Track, len(e.queue))
	copy(queueList, e.queue)

	return map[string]interface{}{
		"state":          string(e.state),
		"volume":         e.volume,
		"elapsed_ms":     elapsedMs,
		"duration_ms":    durationMs,
		"current_track":  curTrackMap,
		"queue":          queueList,
		"queue_count":    len(queueList),
		"queue_length":   len(queueList),
		"history_length": 0,
		"can_skip_back":  false,
	}
}

func (e *AudioEngine) Play(track *db.Track) error {
	e.mu.Lock()
	defer e.mu.Unlock()

	if track != nil {
		e.currentTrack = track
	} else if e.currentTrack == nil && len(e.queue) > 0 {
		e.currentTrack = &e.queue[0]
		e.queue = e.queue[1:]
	} else if e.currentTrack == nil {
		next, err := e.programDirector.SelectNextTrack(nil, nil, nil, nil)
		if err == nil && next != nil {
			e.currentTrack = next
		}
	}

	if e.currentTrack == nil {
		e.state = StateIdle
		if e.player != nil {
			e.player.stop()
		}
		return nil
	}

	_ = e.database.RecordPlay(e.currentTrack.ID, time.Now())
	e.state = StatePlaying

	if e.player != nil {
		e.player.playFile(e.currentTrack.FilePath, e.volume, func() {
			_, _ = e.Skip()
		})
	}

	return nil
}

func (e *AudioEngine) Pause() {
	e.mu.Lock()
	defer e.mu.Unlock()

	if e.state == StatePlaying {
		e.state = StatePaused
		if e.player != nil {
			e.player.pause()
		}
	} else if e.state == StatePaused {
		e.state = StatePlaying
		if e.player != nil {
			e.player.resume()
		}
	}
}

func (e *AudioEngine) SetVolume(vol int) int {
	e.mu.Lock()
	defer e.mu.Unlock()

	if vol < 0 {
		vol = 0
	}
	if vol > 100 {
		vol = 100
	}
	e.volume = vol
	if e.player != nil {
		e.player.setVolume(e.volume)
	}
	return e.volume
}

func (e *AudioEngine) Skip() (*db.Track, error) {
	e.mu.Lock()
	if e.player != nil {
		e.player.stop()
	}
	e.mu.Unlock()

	if len(e.queue) > 0 {
		next := e.queue[0]
		e.queue = e.queue[1:]
		err := e.Play(&next)
		return &next, err
	}

	next, err := e.programDirector.SelectNextTrack(e.currentTrack, nil, nil, nil)
	if err == nil && next != nil {
		errPlay := e.Play(next)
		return next, errPlay
	}

	e.mu.Lock()
	e.state = StateIdle
	e.currentTrack = nil
	e.mu.Unlock()

	return nil, nil
}

func (e *AudioEngine) EnqueueTrack(track *db.Track, playNext bool) {
	if track == nil {
		return
	}
	e.mu.Lock()
	if playNext {
		e.queue = append([]db.Track{*track}, e.queue...)
	} else {
		e.queue = append(e.queue, *track)
	}
	needStart := (e.currentTrack == nil || e.state == StateIdle)
	e.mu.Unlock()

	if needStart {
		_ = e.Play(nil)
	}
}

func (e *AudioEngine) EnqueueAlbum(tracks []db.Track, playNext bool) {
	if len(tracks) == 0 {
		return
	}
	e.mu.Lock()
	if playNext {
		e.queue = append(tracks, e.queue...)
	} else {
		e.queue = append(e.queue, tracks...)
	}
	needStart := (e.currentTrack == nil || e.state == StateIdle)
	e.mu.Unlock()

	if needStart {
		_ = e.Play(nil)
	}
}

func (e *AudioEngine) RemoveQueueItem(index int) bool {
	e.mu.Lock()
	defer e.mu.Unlock()

	if index >= 0 && index < len(e.queue) {
		e.queue = append(e.queue[:index], e.queue[index+1:]...)
		return true
	}
	return false
}

func (e *AudioEngine) MoveQueueItem(fromIndex, toIndex int) bool {
	e.mu.Lock()
	defer e.mu.Unlock()

	if fromIndex >= 0 && fromIndex < len(e.queue) && toIndex >= 0 && toIndex < len(e.queue) {
		item := e.queue[fromIndex]
		e.queue = append(e.queue[:fromIndex], e.queue[fromIndex+1:]...)
		e.queue = append(e.queue[:toIndex], append([]db.Track{item}, e.queue[toIndex:]...)...)
		return true
	}
	return false
}

func (e *AudioEngine) ClearQueue() {
	e.mu.Lock()
	defer e.mu.Unlock()

	e.queue = make([]db.Track, 0)
}
