package server

import (
	"log"
	"net/http"
	"sync"

	"github.com/gorilla/websocket"
	"github.com/mangocats/vaino/pkg/audio"
)

var upgrader = websocket.Upgrader{
	CheckOrigin: func(r *http.Request) bool {
		return true
	},
}

type WSHub struct {
	mu          sync.Mutex
	clients     map[*websocket.Conn]bool
	audioEngine *audio.AudioEngine
}

func NewWSHub(engine *audio.AudioEngine) *WSHub {
	return &WSHub{
		clients:     make(map[*websocket.Conn]bool),
		audioEngine: engine,
	}
}

func (h *WSHub) HandleWS(w http.ResponseWriter, r *http.Request) {
	conn, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		log.Printf("WS upgrade error: %v", err)
		return
	}

	h.mu.Lock()
	h.clients[conn] = true
	h.mu.Unlock()

	defer func() {
		h.mu.Lock()
		delete(h.clients, conn)
		h.mu.Unlock()
		conn.Close()
	}()

	// Send initial status
	_ = conn.WriteJSON(h.audioEngine.GetStatus())

	for {
		var msg map[string]interface{}
		if err := conn.ReadJSON(&msg); err != nil {
			break
		}
	}
}
