package stream

import (
	"fmt"
	"log/slog"
	"sync"
	"time"

	"github.com/google/uuid"
)

type Hub struct {
	mu          sync.RWMutex
	sessions    map[string]*Session
	listenerBuf int
	maxPerUser  int
	ttl         time.Duration
}

func NewHub(listenerBuf, maxPerUser, ttlMinutes int) *Hub {
	h := &Hub{
		sessions:    make(map[string]*Session),
		listenerBuf: listenerBuf,
		maxPerUser:  maxPerUser,
		ttl:         time.Duration(ttlMinutes) * time.Minute,
	}
	go h.reaper()
	return h
}

func (h *Hub) Create(title, visibility, mode, ownerID, token string) (*Session, error) {
	h.mu.Lock()
	defer h.mu.Unlock()

	// enforce per-user stream limit
	count := 0
	for _, s := range h.sessions {
		if s.OwnerID == ownerID {
			count++
		}
	}
	if h.maxPerUser > 0 && count >= h.maxPerUser {
		return nil, fmt.Errorf("stream limit reached")
	}

	id := uuid.New().String()
	s := NewSession(id, title, visibility, mode, ownerID, token, h.listenerBuf)
	h.sessions[id] = s
	slog.Info("[DAWStream] stream created", "id", id, "owner", ownerID)
	return s, nil
}

func (h *Hub) Get(id string) (*Session, bool) {
	h.mu.RLock()
	defer h.mu.RUnlock()
	s, ok := h.sessions[id]
	return s, ok
}

func (h *Hub) Delete(id string) {
	h.mu.Lock()
	defer h.mu.Unlock()
	delete(h.sessions, id)
	slog.Info("[DAWStream] stream removed", "id", id)
}

// reaper periodically removes sessions that have been offline past their TTL.
func (h *Hub) reaper() {
	ticker := time.NewTicker(5 * time.Minute)
	defer ticker.Stop()
	for range ticker.C {
		h.mu.Lock()
		now := time.Now()
		for id, s := range h.sessions {
			s.mu.RLock()
			offline := s.Status == StatusOffline
			stale := now.Sub(s.UpdatedAt) > h.ttl
			s.mu.RUnlock()
			if offline && stale {
				delete(h.sessions, id)
				slog.Info("[DAWStream] stream expired", "id", id)
			}
		}
		h.mu.Unlock()
	}
}
