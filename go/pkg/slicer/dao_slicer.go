package slicer

import (
	"crypto/md5"
	"fmt"
	"log"
	"path/filepath"
	"strings"

	"github.com/mangocats/vaino/pkg/db"
)

type PassageCut struct {
	Index         int
	Title         string
	StartOffsetMs int64
	EndOffsetMs   int64
}

type DAOSlicer struct {
	database *db.Database
}

func NewDAOSlicer(database *db.Database) *DAOSlicer {
	return &DAOSlicer{database: database}
}

func (s *DAOSlicer) SliceAlbumPassages(albumName, artistName string, cuts []PassageCut, sourceFilePath string) ([]string, error) {
	log.Printf("[DAOSlicer] Slicing %d passage tracks for album: %s (%s)", len(cuts), albumName, artistName)

	var createdIDs []string
	for _, cut := range cuts {
		trackID := fmt.Sprintf("%x", md5.Sum([]byte(strings.ToLower(artistName+"||"+albumName+"||"+cut.Title))))[:16]

		t := db.Track{
			ID:            trackID,
			FilePath:      sourceFilePath,
			FileFormat:    strings.TrimPrefix(strings.ToUpper(filepath.Ext(sourceFilePath)), "."),
			Title:         cut.Title,
			Artist:        artistName,
			DurationMs:    cut.EndOffsetMs - cut.StartOffsetMs,
			StartOffsetMs: cut.StartOffsetMs,
		}

		_ = t
		createdIDs = append(createdIDs, trackID)
	}

	return createdIDs, nil
}
