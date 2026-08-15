package scanner

import (
	"crypto/md5"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/dhowden/tag"
	"github.com/mangocats/vaino/pkg/db"
)

type LibraryScanner struct {
	database *db.Database
	workers  int
}

func NewLibraryScanner(database *db.Database) *LibraryScanner {
	return &LibraryScanner{
		database: database,
		workers:  16,
	}
}

func (s *LibraryScanner) ScanDirectory(musicDir string) (int, error) {
	startTime := time.Now()
	log.Printf("[Scanner] Starting directory scan: %s", musicDir)

	var audioFiles []string
	err := filepath.WalkDir(musicDir, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return nil
		}
		if !d.IsDir() {
			ext := strings.ToLower(filepath.Ext(path))
			if ext == ".mp3" || ext == ".flac" || ext == ".m4a" || ext == ".wav" || ext == ".ogg" {
				audioFiles = append(audioFiles, path)
			}
		}
		return nil
	})

	if err != nil {
		return 0, err
	}

	log.Printf("[Scanner] Found %d audio files in %s", len(audioFiles), musicDir)
	if len(audioFiles) == 0 {
		return 0, nil
	}

	fileChan := make(chan string, len(audioFiles))
	for _, f := range audioFiles {
		fileChan <- f
	}
	close(fileChan)

	var wg sync.WaitGroup
	parsedCount := 0
	var mu sync.Mutex

	for i := 0; i < s.workers; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for filePath := range fileChan {
				track, coverBytes, mimeType := s.parseFileMetadata(filePath)
				if track != nil {
					_ = s.database.RecordPlay(track.ID, time.Time{})
					if len(coverBytes) > 0 && track.Album != nil {
						_, _ = s.database.SaveAlbumCoverArt(*track.Album, track.Artist, coverBytes, mimeType, "EMBEDDED")
					}
					mu.Lock()
					parsedCount++
					mu.Unlock()
				}
			}
		}()
	}

	wg.Wait()
	elapsed := time.Since(startTime).Seconds()
	log.Printf("[Scanner] Scan completed in %.2fs! Parsed %d tracks.", elapsed, parsedCount)
	return parsedCount, nil
}

func (s *LibraryScanner) parseFileMetadata(filePath string) (*db.Track, []byte, string) {
	f, err := os.Open(filePath)
	if err != nil {
		return nil, nil, ""
	}
	defer f.Close()

	info, err := f.Stat()
	if err != nil {
		return nil, nil, ""
	}

	ext := strings.TrimPrefix(strings.ToUpper(filepath.Ext(filePath)), ".")
	title := strings.TrimSuffix(filepath.Base(filePath), filepath.Ext(filePath))
	artist := "Unknown Artist"
	album := "Unknown Album"
	var trackNum int64 = 0
	var year int64 = 0

	var coverBytes []byte
	mimeType := "image/jpeg"

	m, err := tag.ReadFrom(f)
	if err == nil {
		if m.Title() != "" {
			title = m.Title()
		}
		if m.Artist() != "" {
			artist = m.Artist()
		}
		if m.Album() != "" {
			album = m.Album()
		}
		tn, _ := m.Track()
		trackNum = int64(tn)
		year = int64(m.Year())

		if pic := m.Picture(); pic != nil {
			coverBytes = pic.Data
			if pic.MIMEType != "" {
				mimeType = pic.MIMEType
			}
		}
	}

	trackID := fmt.Sprintf("%x", md5.Sum([]byte(strings.ToLower(artist+"||"+title+"||"+filePath))))[:16]

	track := &db.Track{
		ID:            trackID,
		FilePath:      filePath,
		FileFormat:    ext,
		Title:         title,
		Artist:        artist,
		DurationMs:    180000,
		StartOffsetMs: 0,
		HasCoverArt:   len(coverBytes) > 0,
		FileMtime:     float64(info.ModTime().Unix()),
		FileSize:      info.Size(),
	}

	if album != "" {
		track.Album = &album
	}
	if trackNum > 0 {
		track.TrackNumber = &trackNum
	}
	if year > 0 {
		track.Year = &year
	}

	return track, coverBytes, mimeType
}
