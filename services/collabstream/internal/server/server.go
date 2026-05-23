package server

import (
	"context"
	"log/slog"
	"net/http"
	"time"

	"futureboard/collabstream/internal/auth"
	"futureboard/collabstream/internal/config"
	"futureboard/collabstream/internal/stream"
)

type Server struct {
	cfg    *config.Config
	hub    *stream.Hub
	http   *http.Server
}

func New(cfg *config.Config) *Server {
	hub := stream.NewHub(cfg.ListenerBuffer, cfg.MaxStreamsPerUser, cfg.StreamTTLMinutes)

	s := &Server{cfg: cfg, hub: hub}

	mux := http.NewServeMux()
	s.registerRoutes(mux)

	authed := auth.Middleware(cfg.JWTSecret, cfg.Mode)

	s.http = &http.Server{
		Addr:         cfg.Addr,
		Handler:      authed(mux),
		ReadTimeout:  30 * time.Second,
		WriteTimeout: 0, // websockets use streaming writes
		IdleTimeout:  120 * time.Second,
	}
	return s
}

func (s *Server) Start() error {
	slog.Info("[DAWStream] listening", "addr", s.cfg.Addr, "mode", s.cfg.Mode, "publicURL", s.cfg.PublicURL)
	return s.http.ListenAndServe()
}

func (s *Server) Shutdown(ctx context.Context) error {
	return s.http.Shutdown(ctx)
}
