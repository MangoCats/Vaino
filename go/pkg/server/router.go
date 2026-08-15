package server

import (
	"encoding/json"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/go-chi/chi/v5/middleware"
	"github.com/go-chi/cors"
	"github.com/mangocats/vaino/pkg/audio"
	"github.com/mangocats/vaino/pkg/db"
	"github.com/mangocats/vaino/pkg/selector"
)

func NewRouter(database *db.Database, engine *audio.AudioEngine) http.Handler {
	r := chi.NewRouter()

	r.Use(middleware.Logger)
	r.Use(middleware.Recoverer)
	r.Use(cors.Handler(cors.Options{
		AllowedOrigins: []string{"*"},
		AllowedMethods: []string{"GET", "POST", "PUT", "DELETE", "OPTIONS"},
		AllowedHeaders: []string{"*"},
	}))

	wsHub := NewWSHub(engine)
	r.Get("/ws", wsHub.HandleWS)

	r.Route("/api/v1", func(r chi.Router) {
		// Player status & controls
		r.Get("/status", func(w http.ResponseWriter, r *http.Request) {
			json.NewEncoder(w).Encode(engine.GetStatus())
		})

		r.Post("/player/play", func(w http.ResponseWriter, r *http.Request) {
			trackID := r.URL.Query().Get("track_id")
			if trackID != "" {
				track, err := database.GetTrackByID(trackID)
				if err == nil && track != nil {
					_ = engine.Play(track)
				} else {
					_ = engine.Play(nil)
				}
			} else {
				_ = engine.Play(nil)
			}
			json.NewEncoder(w).Encode(engine.GetStatus())
		})

		r.Post("/player/pause", func(w http.ResponseWriter, r *http.Request) {
			engine.Pause()
			json.NewEncoder(w).Encode(engine.GetStatus())
		})

		r.Post("/player/skip", func(w http.ResponseWriter, r *http.Request) {
			_, _ = engine.Skip()
			json.NewEncoder(w).Encode(engine.GetStatus())
		})

		r.Post("/player/volume", func(w http.ResponseWriter, r *http.Request) {
			var body struct {
				Volume int `json:"volume"`
			}
			if err := json.NewDecoder(r.Body).Decode(&body); err == nil {
				engine.SetVolume(body.Volume)
			}
			json.NewEncoder(w).Encode(engine.GetStatus())
		})

		// Track & Artist Ratings
		r.Get("/ratings/track/{track_id}", func(w http.ResponseWriter, r *http.Request) {
			trackID := chi.URLParam(r, "track_id")
			ratings, err := database.GetTrackRatings(trackID)
			if err != nil {
				http.Error(w, "Track not found", 404)
				return
			}
			json.NewEncoder(w).Encode(ratings)
		})

		r.Put("/ratings/track/{track_id}", func(w http.ResponseWriter, r *http.Request) {
			trackID := chi.URLParam(r, "track_id")
			var body struct {
				Rotation  float64 `json:"rotation"`
				Recovery  float64 `json:"recovery"`
				Restraint float64 `json:"restraint"`
				Profanity float64 `json:"profanity"`
				Occasions *string `json:"occasions"`
			}
			if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
				http.Error(w, err.Error(), 400)
				return
			}
			res, err := database.UpdateTrackRatings(trackID, body.Rotation, body.Recovery, body.Restraint, body.Profanity, body.Occasions)
			if err != nil {
				http.Error(w, err.Error(), 500)
				return
			}
			json.NewEncoder(w).Encode(res)
		})

		r.Get("/ratings/artist/{artist_name}", func(w http.ResponseWriter, r *http.Request) {
			artistName := chi.URLParam(r, "artist_name")
			ratings, err := database.GetArtistRatings(artistName)
			if err != nil {
				http.Error(w, err.Error(), 500)
				return
			}
			json.NewEncoder(w).Encode(ratings)
		})

		r.Put("/ratings/artist/{artist_name}", func(w http.ResponseWriter, r *http.Request) {
			artistName := chi.URLParam(r, "artist_name")
			var body struct {
				Rotation     float64 `json:"rotation"`
				Recovery     float64 `json:"recovery"`
				Restraint    float64 `json:"restraint"`
				StreakLength float64 `json:"streak_length"`
			}
			if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
				http.Error(w, err.Error(), 400)
				return
			}
			res, err := database.UpdateArtistRatings(artistName, body.Rotation, body.Recovery, body.Restraint, body.StreakLength)
			if err != nil {
				http.Error(w, err.Error(), 500)
				return
			}
			json.NewEncoder(w).Encode(res)
		})

		r.Get("/ratings/artists", func(w http.ResponseWriter, r *http.Request) {
			ratings, err := database.GetAllArtistRatings()
			if err != nil {
				http.Error(w, err.Error(), 500)
				return
			}
			json.NewEncoder(w).Encode(ratings)
		})

		r.Post("/ratings/import-mulib", func(w http.ResponseWriter, r *http.Request) {
			var body struct {
				MulibPath string `json:"mulib_path"`
			}
			_ = json.NewDecoder(r.Body).Decode(&body)
			if body.MulibPath == "" {
				body.MulibPath = `C:\Users\Mango Cat\Dev\MuLibPlay\mulib.db`
			}
			res, err := database.ImportMulibPreferences(body.MulibPath)
			if err != nil {
				http.Error(w, err.Error(), 400)
				return
			}
			json.NewEncoder(w).Encode(res)
		})

		// Library Navigation
		r.Get("/library/tracks", func(w http.ResponseWriter, r *http.Request) {
			limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
			if limit <= 0 {
				limit = 100
			}
			offset, _ := strconv.Atoi(r.URL.Query().Get("offset"))
			query := r.URL.Query().Get("query")
			artist := r.URL.Query().Get("artist")
			album := r.URL.Query().Get("album")
			letter := r.URL.Query().Get("letter")

			total, _ := database.GetTotalTrackCount(query, artist, album, letter)
			tracks, err := database.GetAllTracks(limit, offset, query, artist, album, letter)
			if err != nil {
				http.Error(w, err.Error(), 500)
				return
			}
			json.NewEncoder(w).Encode(map[string]interface{}{
				"tracks": tracks,
				"total":  total,
			})
		})

		r.Get("/library/albums", func(w http.ResponseWriter, r *http.Request) {
			limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
			if limit <= 0 {
				limit = 100
			}
			offset, _ := strconv.Atoi(r.URL.Query().Get("offset"))
			query := r.URL.Query().Get("query")
			artist := r.URL.Query().Get("artist")
			letter := r.URL.Query().Get("letter")

			total, _ := database.GetTotalAlbumCount(query, artist, letter)
			albums, err := database.GetAllAlbums(limit, offset, query, artist, letter)
			if err != nil {
				http.Error(w, err.Error(), 500)
				return
			}
			json.NewEncoder(w).Encode(map[string]interface{}{
				"albums": albums,
				"total":  total,
			})
		})

		r.Get("/library/artists", func(w http.ResponseWriter, r *http.Request) {
			limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
			if limit <= 0 {
				limit = 100
			}
			offset, _ := strconv.Atoi(r.URL.Query().Get("offset"))
			query := r.URL.Query().Get("query")
			letter := r.URL.Query().Get("letter")

			total, _ := database.GetTotalArtistCount(query, letter)
			artists, err := database.GetAllArtists(limit, offset, query, letter)
			if err != nil {
				http.Error(w, err.Error(), 500)
				return
			}
			json.NewEncoder(w).Encode(map[string]interface{}{
				"artists": artists,
				"total":   total,
			})
		})

		r.Get("/library/albums/{album_name}/tracks", func(w http.ResponseWriter, r *http.Request) {
			albumName := chi.URLParam(r, "album_name")
			if unescaped, err := url.QueryUnescape(albumName); err == nil {
				albumName = unescaped
			}
			artistName := r.URL.Query().Get("artist")

			tracks, err := database.GetAlbumTracks(albumName, artistName)
			if err != nil {
				http.Error(w, err.Error(), 500)
				return
			}
			json.NewEncoder(w).Encode(map[string]interface{}{
				"tracks": tracks,
				"total":  len(tracks),
			})
		})

		r.Get("/art/{id}", func(w http.ResponseWriter, r *http.Request) {
			id := chi.URLParam(r, "id")
			imgData, mimeType, err := database.GetCoverArt(id)
			if err == nil && len(imgData) > 0 {
				w.Header().Set("Content-Type", mimeType)
				w.Write(imgData)
				return
			}

			// SVG Placeholder Fallback
			svgPlaceholder := `<svg xmlns="http://www.w3.org/2000/svg" width="300" height="300" viewBox="0 0 300 300">
				<rect width="300" height="300" fill="#1e2230"/>
				<text x="50%" y="50%" dominant-baseline="middle" text-anchor="middle" fill="#4a5568" font-size="48">🎵</text>
			</svg>`
			w.Header().Set("Content-Type", "image/svg+xml")
			w.Write([]byte(svgPlaceholder))
		})

		r.Get("/descriptors/{track_id}", func(w http.ResponseWriter, r *http.Request) {
			trackID := chi.URLParam(r, "track_id")
			desc, _ := database.GetTrackDescriptors(trackID)
			track, _ := database.GetTrackByID(trackID)
			json.NewEncoder(w).Encode(map[string]interface{}{
				"track":       track,
				"descriptors": desc,
				"is_fallback": desc == nil,
			})
		})

		r.Post("/queue/add", func(w http.ResponseWriter, r *http.Request) {
			var body struct {
				TrackID   string `json:"track_id"`
				AlbumName string `json:"album_name"`
				PlayNext  bool   `json:"play_next"`
			}
			if err := json.NewDecoder(r.Body).Decode(&body); err == nil {
				if body.TrackID != "" {
					track, errTrack := database.GetTrackByID(body.TrackID)
					if errTrack == nil && track != nil {
						engine.EnqueueTrack(track, body.PlayNext)
					}
				} else if body.AlbumName != "" {
					tracks, errAlbum := database.GetAlbumTracks(body.AlbumName, "")
					if errAlbum == nil && len(tracks) > 0 {
						engine.EnqueueAlbum(tracks, body.PlayNext)
					}
				}
			}
			json.NewEncoder(w).Encode(engine.GetStatus())
		})
		r.Post("/queue/move", func(w http.ResponseWriter, r *http.Request) {
			var body struct {
				FromIndex int `json:"from_index"`
				ToIndex   int `json:"to_index"`
			}
			if err := json.NewDecoder(r.Body).Decode(&body); err == nil {
				engine.MoveQueueItem(body.FromIndex, body.ToIndex)
			}
			json.NewEncoder(w).Encode(engine.GetStatus())
		})
		r.Delete("/queue/remove/{index}", func(w http.ResponseWriter, r *http.Request) {
			if idx, err := strconv.Atoi(chi.URLParam(r, "index")); err == nil {
				engine.RemoveQueueItem(idx)
			}
			json.NewEncoder(w).Encode(engine.GetStatus())
		})
		r.Delete("/queue/clear", func(w http.ResponseWriter, r *http.Request) {
			engine.ClearQueue()
			json.NewEncoder(w).Encode(engine.GetStatus())
		})
		r.Post("/player/previous", func(w http.ResponseWriter, r *http.Request) {
			json.NewEncoder(w).Encode(engine.GetStatus())
		})

		r.Get("/programs", func(w http.ResponseWriter, r *http.Request) {
			progs, err := database.GetAllPrograms()
			if err != nil || len(progs) == 0 {
				_, _ = database.ImportMuLibPrograms("")
				progs, _ = database.GetAllPrograms()
			}
			pd := selector.NewProgramDirector(database)
			activeProg, _ := pd.GetActiveProgram(time.Now())
			activeID := int64(0)
			if activeProg != nil {
				activeID = activeProg.ID
			} else if len(progs) > 0 {
				activeID = progs[0].ID
			}

			results := make([]map[string]interface{}, 0, len(progs))
			for _, p := range progs {
				tVec := pd.ComputeProgramTargetVector(&p)
				pMap := map[string]interface{}{
					"id":            p.ID,
					"name":          p.Name,
					"start_time":    p.StartTime,
					"track_ids":     p.TrackIDs,
					"is_active":     p.ID == activeID,
					"target_vector": tVec,
				}
				results = append(results, pMap)
			}

			json.NewEncoder(w).Encode(map[string]interface{}{
				"programs":             results,
				"active_program_id":   activeID,
				"use_clock_autoselect": true,
			})
		})
		r.Post("/programs", func(w http.ResponseWriter, r *http.Request) {
			var body struct {
				Name      string `json:"name"`
				StartTime string `json:"start_time"`
				TrackIDs  string `json:"track_ids"`
			}
			if err := json.NewDecoder(r.Body).Decode(&body); err == nil {
				p, errSave := database.SaveProgram(body.Name, body.StartTime, body.TrackIDs)
				if errSave == nil {
					json.NewEncoder(w).Encode(p)
					return
				}
			}
			http.Error(w, "Failed to save program", 400)
		})
		r.Put("/programs/{id}", func(w http.ResponseWriter, r *http.Request) {
			id, _ := strconv.ParseInt(chi.URLParam(r, "id"), 10, 64)
			var body struct {
				Name      string `json:"name"`
				StartTime string `json:"start_time"`
				TrackIDs  string `json:"track_ids"`
			}
			if err := json.NewDecoder(r.Body).Decode(&body); err == nil {
				p, errUp := database.UpdateProgram(id, body.Name, body.StartTime, body.TrackIDs)
				if errUp == nil {
					json.NewEncoder(w).Encode(p)
					return
				}
			}
			http.Error(w, "Failed to update program", 400)
		})
		r.Delete("/programs/{id}", func(w http.ResponseWriter, r *http.Request) {
			id, _ := strconv.ParseInt(chi.URLParam(r, "id"), 10, 64)
			_ = database.DeleteProgram(id)
			json.NewEncoder(w).Encode(map[string]interface{}{"status": "deleted"})
		})
		r.Post("/programs/import-mulib", func(w http.ResponseWriter, r *http.Request) {
			count, _ := database.ImportMuLibPrograms("")
			progs, _ := database.GetAllPrograms()
			json.NewEncoder(w).Encode(map[string]interface{}{
				"imported": count,
				"programs": progs,
			})
		})
		r.Post("/programs/toggle-autoselect", func(w http.ResponseWriter, r *http.Request) {
			json.NewEncoder(w).Encode(map[string]interface{}{"status": "ok", "use_clock_autoselect": true})
		})
	})

	// Static Web UI File Server
	webDirs := []string{
		"src/web",
		"../src/web",
		"./web",
		"../web",
	}

	var foundWebDir string
	for _, d := range webDirs {
		if _, err := os.Stat(filepath.Join(d, "index.html")); err == nil {
			foundWebDir = d
			break
		}
	}

	if foundWebDir != "" {
		fs := http.FileServer(http.Dir(foundWebDir))
		r.Get("/*", func(w http.ResponseWriter, r *http.Request) {
			if _, err := os.Stat(filepath.Join(foundWebDir, r.URL.Path)); os.IsNotExist(err) && r.URL.Path != "/" {
				http.ServeFile(w, r, filepath.Join(foundWebDir, "index.html"))
				return
			}
			fs.ServeHTTP(w, r)
		})
	}

	return r
}
