//go:build windows

package audio

import (
	"log"
	"os"
	"time"

	"github.com/ebitengine/oto/v3"
	"github.com/hajimehoshi/go-mp3"
)

type pcmPlayer struct {
	otoCtx     *oto.Context
	player     *oto.Player
	fileHandle *os.File
	stopChan   chan struct{}
	startTime  time.Time
}

func newPCMPlayer() *pcmPlayer {
	op := &oto.NewContextOptions{
		SampleRate:   44100,
		ChannelCount: 2,
		Format:       oto.FormatSignedInt16LE,
	}
	otoCtx, ready, err := oto.NewContext(op)
	if err != nil {
		log.Printf("[AudioEngine] Warning: Could not initialize Windows PCM audio context: %v", err)
		return &pcmPlayer{}
	}
	<-ready
	log.Printf("[AudioEngine] Windows DirectSound/WASAPI PCM Audio Output initialized successfully.")
	return &pcmPlayer{otoCtx: otoCtx}
}

func (p *pcmPlayer) stop() {
	p.startTime = time.Time{}
	if p.stopChan != nil {
		close(p.stopChan)
		p.stopChan = nil
	}
	if p.player != nil {
		p.player.Close()
		p.player = nil
	}
	if p.fileHandle != nil {
		p.fileHandle.Close()
		p.fileHandle = nil
	}
}

func (p *pcmPlayer) playFile(filePath string, volume int, onFinished func()) {
	p.stop()

	if p.otoCtx == nil || filePath == "" {
		return
	}

	file, err := os.Open(filePath)
	if err != nil {
		log.Printf("[AudioEngine] Track file not found: %s", filePath)
		return
	}
	p.fileHandle = file

	decodedMp3, errMp3 := mp3.NewDecoder(file)
	if errMp3 != nil {
		log.Printf("[AudioEngine] Error decoding MP3 stream: %v", errMp3)
		file.Close()
		p.fileHandle = nil
		return
	}

	player := p.otoCtx.NewPlayer(decodedMp3)
	player.SetVolume(float64(volume) / 100.0)
	player.Play()
	p.player = player
	p.startTime = time.Now()
	p.stopChan = make(chan struct{})

	stopChan := p.stopChan
	go func() {
		for {
			select {
			case <-stopChan:
				return
			default:
				if player != nil && !player.IsPlaying() && player.BufferedSize() == 0 {
					if onFinished != nil {
						onFinished()
					}
					return
				}
				time.Sleep(200 * time.Millisecond)
			}
		}
	}()
}

func (p *pcmPlayer) pause() {
	if p.player != nil {
		p.player.Pause()
	}
}

func (p *pcmPlayer) resume() {
	if p.player != nil {
		p.player.Play()
	}
}

func (p *pcmPlayer) setVolume(vol int) {
	if p.player != nil {
		p.player.SetVolume(float64(vol) / 100.0)
	}
}

func (p *pcmPlayer) getPositionMs() int64 {
	if p.startTime.IsZero() {
		return 0
	}
	return time.Since(p.startTime).Milliseconds()
}
