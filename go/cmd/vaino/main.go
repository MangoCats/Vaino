package main

import (
	"flag"
	"fmt"
	"log"
	"net/http"
	"os"

	"github.com/mangocats/vaino/pkg/audio"
	"github.com/mangocats/vaino/pkg/db"
	"github.com/mangocats/vaino/pkg/server"
)

func main() {
	dbPathFlag := flag.String("db", "vaino_go.db", "Path to SQLite database file")
	portFlag := flag.Int("port", 8000, "HTTP server listening port")
	flag.Parse()

	targetDbPath := *dbPathFlag

	// Auto-detect master database in working dir or parent dir
	if targetDbPath == "vaino_go.db" || targetDbPath == "" {
		candidates := []string{"../vaino.db", "vaino.db", "vaino_go.db"}
		for _, c := range candidates {
			if fi, err := os.Stat(c); err == nil && fi.Size() > 100000 {
				targetDbPath = c
				break
			}
		}
	}

	log.Printf("Starting Vaino (Go Edition v0.1.0)...")
	log.Printf("Resolved Database Path: %s", targetDbPath)

	database, err := db.NewDatabase(targetDbPath)
	if err != nil {
		log.Fatalf("Failed to initialize database: %v", err)
	}
	defer database.Close()

	audioEngine := audio.NewAudioEngine(database)
	router := server.NewRouter(database, audioEngine)

	addr := fmt.Sprintf(":%d", *portFlag)
	log.Printf("Vaino Go REST API listening on http://localhost%s", addr)
	if err := http.ListenAndServe(addr, router); err != nil {
		log.Fatalf("Server failed: %v", err)
	}
}
