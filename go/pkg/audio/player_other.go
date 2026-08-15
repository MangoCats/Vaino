//go:build !windows

package audio

import (
	"log"
	"os"
	"os/exec"
	"time"
)

type pcmPlayer struct {
	cmd       *exec.Cmd
	startTime time.Time
}

func newPCMPlayer() *pcmPlayer {
	log.Printf("[AudioEngine] POSIX/Linux ALSA Audio Output initialized.")
	return &pcmPlayer{}
}

func (p *pcmPlayer) stop() {
	p.startTime = time.Time{}
	if p.cmd != nil && p.cmd.Process != nil {
		_ = p.cmd.Process.Kill()
		p.cmd = nil
	}
}

func (p *pcmPlayer) playFile(filePath string, volume int, onFinished func()) {
	p.stop()
	if filePath == "" {
		return
	}
	p.startTime = time.Now()
	cmd := exec.Command("mpg123", "-q", filePath)
	if err := cmd.Start(); err == nil {
		p.cmd = cmd
		go func() {
			_ = cmd.Wait()
			if onFinished != nil {
				onFinished()
			}
		}()
	} else {
		log.Printf("[AudioEngine] Notice: mpg123 player not found, track registered for playback: %s", filePath)
	}
}

func (p *pcmPlayer) pause() {
	if p.cmd != nil && p.cmd.Process != nil {
		_ = p.cmd.Process.Signal(os.Interrupt)
	}
}

func (p *pcmPlayer) resume() {
}

func (p *pcmPlayer) setVolume(vol int) {
}

func (p *pcmPlayer) getPositionMs() int64 {
	if p.startTime.IsZero() {
		return 0
	}
	return time.Since(p.startTime).Milliseconds()
}
