package server

import (
	"context"
	"encoding/json"
	"log/slog"
	"net/http"
	"time"

	"futureboard/collabstream/internal/stream"

	"nhooyr.io/websocket"
)

const wsIdleTimeout = 5 * time.Minute

// handlePublish accepts a publisher WebSocket connection.
// URL: GET /ws/publish/{uuid}?token=...
func (s *Server) handlePublish(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("uuid")
	token := r.URL.Query().Get("token")

	sess, ok := s.hub.Get(id)
	if !ok {
		http.Error(w, "stream not found", http.StatusNotFound)
		return
	}

	if token == "" || token != sess.Token {
		slog.Warn("[DAWStream] publish rejected: invalid token", "id", id)
		http.Error(w, "unauthorized", http.StatusForbidden)
		return
	}

	conn, err := websocket.Accept(w, r, &websocket.AcceptOptions{
		InsecureSkipVerify: true, // handled by reverse proxy / CORS config
	})
	if err != nil {
		slog.Error("[DAWStream] websocket accept error", "err", err)
		return
	}
	defer conn.CloseNow()

	pub := stream.NewClient(0) // publisher does not receive audio back
	if !sess.SetPublisher(pub) {
		conn.Close(websocket.StatusPolicyViolation, "another publisher is already active")
		slog.Warn("[DAWStream] duplicate publisher rejected", "id", id)
		return
	}
	defer func() {
		sess.RemovePublisher()
		// notify all listeners that stream went offline
		sess.BroadcastControl(stream.NewStatusMsg("offline", sess.ListenerCount()))
		slog.Info("[DAWStream] publisher disconnected", "id", id)
	}()

	slog.Info("[DAWStream] publisher connected", "id", id)

	ctx, cancel := context.WithCancel(r.Context())
	defer cancel()

	conn.SetReadLimit(int64(s.cfg.MaxFrameBytes))

	for {
		msgType, data, err := conn.Read(ctx)
		if err != nil {
			return
		}

		switch msgType {
		case websocket.MessageText:
			s.handlePublisherControl(sess, data)
		case websocket.MessageBinary:
			sess.Broadcast(data)
		}
	}
}

func (s *Server) handlePublisherControl(sess *stream.Session, data []byte) {
	var msg stream.PublisherMsg
	if err := json.Unmarshal(data, &msg); err != nil {
		slog.Warn("[DAWStream] invalid control message from publisher", "id", sess.ID)
		return
	}

	switch msg.Type {
	case stream.MsgStreamStart:
		cfg := stream.StreamConfig{
			Codec:      msg.Codec,
			SampleRate: msg.SampleRate,
			Channels:   msg.Channels,
			FrameMs:    msg.FrameMs,
		}
		if cfg.Codec == "" {
			cfg.Codec = "pcm-f32"
		}
		if cfg.SampleRate == 0 {
			cfg.SampleRate = 48000
		}
		if cfg.Channels == 0 {
			cfg.Channels = 2
		}
		if cfg.FrameMs == 0 {
			cfg.FrameMs = 20
		}
		if msg.Title != "" {
			sess.Title = msg.Title
		}
		sess.UpdateConfig(cfg)
		// broadcast stream:info then stream:status to all listeners
		sess.BroadcastControl(stream.NewInfoMsg(sess))
		sess.BroadcastControl(stream.NewStatusMsg("live", sess.ListenerCount()))
		slog.Info("[DAWStream] stream started", "id", sess.ID, "codec", cfg.Codec)

	case stream.MsgStreamStop:
		sess.BroadcastControl(stream.NewStatusMsg("offline", sess.ListenerCount()))
		slog.Info("[DAWStream] stream stopped by publisher", "id", sess.ID)

	case stream.MsgStreamPing:
		// no-op keepalive
	}
}

// handleListen accepts a listener WebSocket connection.
// URL: GET /ws/listen/{uuid}
func (s *Server) handleListen(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("uuid")

	sess, ok := s.hub.Get(id)
	if !ok {
		http.Error(w, "stream not found", http.StatusNotFound)
		return
	}

	if s.cfg.MaxListenersPerStream > 0 && sess.ListenerCount() >= s.cfg.MaxListenersPerStream {
		http.Error(w, "listener limit reached", http.StatusServiceUnavailable)
		return
	}

	conn, err := websocket.Accept(w, r, &websocket.AcceptOptions{
		InsecureSkipVerify: true,
	})
	if err != nil {
		slog.Error("[DAWStream] listener websocket accept error", "err", err)
		return
	}
	defer conn.CloseNow()

	client := stream.NewClient(s.cfg.ListenerBuffer)
	sess.AddListener(client)
	defer func() {
		sess.RemoveListener(client)
		client.Close()
		count := sess.ListenerCount()
		sess.BroadcastControl(stream.NewStatusMsg(string(sess.Status), count))
		slog.Info("[DAWStream] listener disconnected", "id", id, "count", count)
	}()

	count := sess.ListenerCount()
	slog.Info("[DAWStream] listener joined", "id", id, "count", count)

	ctx, cancel := context.WithCancel(r.Context())
	defer cancel()

	// send current stream info + status immediately
	if err := conn.Write(ctx, websocket.MessageText, stream.NewInfoMsg(sess)); err != nil {
		return
	}
	status := "offline"
	if sess.Status == "live" {
		status = "live"
	}
	if err := conn.Write(ctx, websocket.MessageText, stream.NewStatusMsg(status, count)); err != nil {
		return
	}

	// announce new listener count to all others
	sess.BroadcastControl(stream.NewStatusMsg(string(sess.Status), count))

	conn.SetReadLimit(512) // listeners only send keepalive pings, tiny messages

	// fan-out loop: forward frames from hub channel to this websocket
	for {
		select {
		case <-ctx.Done():
			return
		case <-client.Done():
			return
		case frame, open := <-client.Recv():
			if !open {
				return
			}
			// control frames are tagged with leading 0x00 byte
			if len(frame) > 0 && frame[0] == 0x00 {
				if err := conn.Write(ctx, websocket.MessageText, frame[1:]); err != nil {
					return
				}
			} else {
				if err := conn.Write(ctx, websocket.MessageBinary, frame); err != nil {
					return
				}
			}
		}
	}
}
