package stream

import (
	"sync"
	"time"
)

type Status string

const (
	StatusOffline Status = "offline"
	StatusLive    Status = "live"
)

type StreamConfig struct {
	Codec      string
	SampleRate int
	Channels   int
	FrameMs    int
}

type Client struct {
	// ch receives binary audio frames and control JSON frames.
	// Text frames (control) are sent with a leading 0x00 marker byte so
	// the websocket goroutine can distinguish them from audio.
	ch   chan []byte
	done chan struct{}
}

func NewClient(bufSize int) *Client {
	return &Client{
		ch:   make(chan []byte, bufSize),
		done: make(chan struct{}),
	}
}

func (c *Client) Send(frame []byte) bool {
	select {
	case c.ch <- frame:
		return true
	default:
		return false // buffer full — drop frame
	}
}

func (c *Client) Recv() <-chan []byte { return c.ch }
func (c *Client) Done() <-chan struct{} { return c.done }
func (c *Client) Close()               { close(c.done) }

type Session struct {
	mu        sync.RWMutex
	ID        string
	Title     string
	Visibility string
	Mode      string
	OwnerID   string
	Token     string
	Status    Status
	Publisher *Client
	Listeners map[*Client]bool
	Config    StreamConfig
	CreatedAt time.Time
	UpdatedAt time.Time
}

func NewSession(id, title, visibility, mode, ownerID, token string, listenerBuf int) *Session {
	return &Session{
		ID:          id,
		Title:       title,
		Visibility:  visibility,
		Mode:        mode,
		OwnerID:     ownerID,
		Token:       token,
		Status:      StatusOffline,
		Listeners:   make(map[*Client]bool),
		Config:      StreamConfig{Codec: "pcm-f32", SampleRate: 48000, Channels: 2, FrameMs: 20},
		CreatedAt:   time.Now(),
		UpdatedAt:   time.Now(),
	}
}

func (s *Session) ListenerCount() int {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return len(s.Listeners)
}

func (s *Session) SetPublisher(pub *Client) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.Publisher != nil {
		return false // already has active publisher
	}
	s.Publisher = pub
	s.Status = StatusLive
	s.UpdatedAt = time.Now()
	return true
}

func (s *Session) RemovePublisher() {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.Publisher = nil
	s.Status = StatusOffline
	s.UpdatedAt = time.Now()
}

func (s *Session) AddListener(c *Client) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.Listeners[c] = true
	s.UpdatedAt = time.Now()
}

func (s *Session) RemoveListener(c *Client) {
	s.mu.Lock()
	defer s.mu.Unlock()
	delete(s.Listeners, c)
	s.UpdatedAt = time.Now()
}

// Broadcast sends a binary audio frame to all listeners without blocking.
// Slow listeners have frames dropped silently.
func (s *Session) Broadcast(frame []byte) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	for c := range s.Listeners {
		c.Send(frame)
	}
}

// BroadcastControl sends a JSON control message to all listeners.
func (s *Session) BroadcastControl(msg []byte) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	for c := range s.Listeners {
		// tag control frames so the listener goroutine routes them as text
		tagged := make([]byte, len(msg)+1)
		tagged[0] = 0x00 // control marker
		copy(tagged[1:], msg)
		c.Send(tagged)
	}
}

func (s *Session) UpdateConfig(cfg StreamConfig) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.Config = cfg
	if cfg.Codec != "" {
		s.Config = cfg
	}
}

func (s *Session) Snapshot() SessionSnapshot {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return SessionSnapshot{
		ID:         s.ID,
		Title:      s.Title,
		Status:     string(s.Status),
		Listeners:  len(s.Listeners),
		Codec:      s.Config.Codec,
		SampleRate: s.Config.SampleRate,
		Channels:   s.Config.Channels,
		CreatedAt:  s.CreatedAt,
	}
}

type SessionSnapshot struct {
	ID         string    `json:"id"`
	Title      string    `json:"title"`
	Status     string    `json:"status"`
	Listeners  int       `json:"listeners"`
	Codec      string    `json:"codec"`
	SampleRate int       `json:"sampleRate"`
	Channels   int       `json:"channels"`
	CreatedAt  time.Time `json:"createdAt"`
}
