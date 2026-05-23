package config

import (
	"fmt"
	"os"
	"strconv"
)

type Config struct {
	Mode      string
	Addr      string
	PublicURL string
	Codec     string
	AuthMode  string
	JWTSecret string

	MaxStreamsPerUser     int
	MaxListenersPerStream int
	MaxFrameBytes        int
	ListenerBuffer       int
	StreamTTLMinutes     int
}

func Load() (*Config, error) {
	c := &Config{
		Mode:                 getEnv("DAWSTREAM_MODE", "embedded"),
		Addr:                 getEnv("DAWSTREAM_ADDR", "127.0.0.1:8787"),
		PublicURL:            getEnv("DAWSTREAM_PUBLIC_URL", "http://127.0.0.1:8787"),
		Codec:                getEnv("DAWSTREAM_CODEC", "pcm-f32"),
		AuthMode:             getEnv("DAWSTREAM_AUTH_MODE", ""),
		JWTSecret:            getEnv("DAWSTREAM_JWT_SECRET", ""),
		MaxStreamsPerUser:     getEnvInt("DAWSTREAM_MAX_STREAMS_PER_USER", 3),
		MaxListenersPerStream: getEnvInt("DAWSTREAM_MAX_LISTENERS_PER_STREAM", 64),
		MaxFrameBytes:        getEnvInt("DAWSTREAM_MAX_FRAME_BYTES", 65536),
		ListenerBuffer:       getEnvInt("DAWSTREAM_LISTENER_BUFFER", 128),
		StreamTTLMinutes:     getEnvInt("DAWSTREAM_STREAM_TTL_MINUTES", 180),
	}

	if c.Mode != "embedded" && c.Mode != "central" {
		return nil, fmt.Errorf("invalid DAWSTREAM_MODE: %q (must be embedded or central)", c.Mode)
	}

	if c.Mode == "central" && c.JWTSecret == "" {
		return nil, fmt.Errorf("DAWSTREAM_JWT_SECRET is required in central mode")
	}

	return c, nil
}

func getEnv(key, def string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return def
}

func getEnvInt(key string, def int) int {
	if v := os.Getenv(key); v != "" {
		n, err := strconv.Atoi(v)
		if err == nil {
			return n
		}
	}
	return def
}
