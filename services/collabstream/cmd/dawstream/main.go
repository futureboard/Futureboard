package main

import (
	"context"
	"errors"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"futureboard/collabstream/internal/config"
	"futureboard/collabstream/internal/server"
)

func main() {
	cfg, err := config.Load()
	if err != nil {
		slog.Error("[DAWStream] config error", "err", err)
		os.Exit(1)
	}

	srv := server.New(cfg)

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	go func() {
		if err := srv.Start(); err != nil && !errors.Is(err, http.ErrServerClosed) {
			slog.Error("[DAWStream] server error", "err", err)
			stop()
		}
	}()

	<-ctx.Done()
	slog.Info("[DAWStream] shutting down…")

	shutCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := srv.Shutdown(shutCtx); err != nil {
		slog.Error("[DAWStream] shutdown error", "err", err)
	}
	slog.Info("[DAWStream] stopped")
}
