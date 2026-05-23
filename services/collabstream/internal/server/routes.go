package server

import (
	"encoding/json"
	"fmt"
	"log/slog"
	"net/http"
	"strings"
	"time"

	"futureboard/collabstream/internal/auth"
	"futureboard/collabstream/internal/web"
)

const version = "0.1.0"

func (s *Server) registerRoutes(mux *http.ServeMux) {
	mux.HandleFunc("GET /", s.handleIndex)
	mux.HandleFunc("GET /health", s.handleHealth)

	mux.HandleFunc("POST /api/streams", s.handleCreateStream)
	mux.HandleFunc("GET /api/streams/{uuid}", s.handleGetStream)

	mux.HandleFunc("POST /api/auth/login", s.handleLogin)
	mux.HandleFunc("POST /api/auth/logout", s.handleLogout)
	mux.HandleFunc("GET /api/auth/me", s.handleMe)

	mux.HandleFunc("GET /listen/{uuid}", s.handleListenPage)

	mux.HandleFunc("GET /ws/publish/{uuid}", s.handlePublish)
	mux.HandleFunc("GET /ws/listen/{uuid}", s.handleListen)
}

// ---- index / health ---------------------------------------------------

func (s *Server) handleIndex(w http.ResponseWriter, r *http.Request) {
	if r.URL.Path != "/" {
		http.NotFound(w, r)
		return
	}
	writeJSON(w, http.StatusOK, map[string]string{
		"service": "DAWStream",
		"version": version,
		"mode":    s.cfg.Mode,
	})
}

func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{
		"ok":      true,
		"service": "DAWStream",
		"version": version,
		"mode":    s.cfg.Mode,
	})
}

// ---- stream management ------------------------------------------------

type createStreamReq struct {
	Title      string `json:"title"`
	Visibility string `json:"visibility"`
}

type createStreamResp struct {
	ID           string `json:"id"`
	Title        string `json:"title"`
	ListenURL    string `json:"listenUrl"`
	PublishURL   string `json:"publishUrl"`
	PublishToken string `json:"publishToken"`
	Mode         string `json:"mode"`
}

func (s *Server) handleCreateStream(w http.ResponseWriter, r *http.Request) {
	user := auth.FromContext(r.Context())

	// central mode requires authenticated broadcaster
	if s.cfg.Mode == "central" && user == nil {
		writeErr(w, http.StatusUnauthorized, "login required to create a stream")
		return
	}

	var req createStreamReq
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeErr(w, http.StatusBadRequest, "invalid request body")
		return
	}
	if req.Title == "" {
		req.Title = "Untitled Stream"
	}
	if req.Visibility == "" {
		req.Visibility = "private"
	}

	ownerID := "anonymous"
	if user != nil {
		ownerID = user.ID
	}

	token := auth.NewPublishToken()

	sess, err := s.hub.Create(req.Title, req.Visibility, s.cfg.Mode, ownerID, token)
	if err != nil {
		writeErr(w, http.StatusTooManyRequests, err.Error())
		return
	}

	pub := s.cfg.PublicURL
	wsScheme := "ws"
	if strings.HasPrefix(pub, "https://") {
		wsScheme = "wss"
		pub = strings.TrimPrefix(pub, "https://")
	} else {
		pub = strings.TrimPrefix(pub, "http://")
	}

	writeJSON(w, http.StatusCreated, createStreamResp{
		ID:           sess.ID,
		Title:        sess.Title,
		ListenURL:    fmt.Sprintf("%s/listen/%s", s.cfg.PublicURL, sess.ID),
		PublishURL:   fmt.Sprintf("%s://%s/ws/publish/%s", wsScheme, pub, sess.ID),
		PublishToken: token,
		Mode:         s.cfg.Mode,
	})

	slog.Info("[DAWStream] stream created via API", "id", sess.ID, "owner", ownerID)
}

func (s *Server) handleGetStream(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("uuid")
	sess, ok := s.hub.Get(id)
	if !ok {
		writeErr(w, http.StatusNotFound, "stream not found")
		return
	}
	writeJSON(w, http.StatusOK, sess.Snapshot())
}

// ---- auth endpoints (central mode) ------------------------------------

type loginReq struct {
	Email    string `json:"email"`
	Password string `json:"password"`
	Name     string `json:"name"`
}

func (s *Server) handleLogin(w http.ResponseWriter, r *http.Request) {
	if s.cfg.Mode != "central" {
		writeErr(w, http.StatusNotFound, "auth not available in embedded mode")
		return
	}
	var req loginReq
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil || req.Email == "" {
		writeErr(w, http.StatusBadRequest, "invalid login request")
		return
	}

	// stub: accept any non-empty email/password and issue a token.
	// Replace with real account service in production.
	user := auth.AuthUser{ID: req.Email, Email: req.Email, Name: req.Name}
	if user.Name == "" {
		user.Name = req.Email
	}
	token, err := auth.IssueToken(user, s.cfg.JWTSecret, 24*time.Hour)
	if err != nil {
		writeErr(w, http.StatusInternalServerError, "could not issue token")
		return
	}

	http.SetCookie(w, &http.Cookie{
		Name:     "dawstream_session",
		Value:    token,
		Path:     "/",
		HttpOnly: true,
		SameSite: http.SameSiteLaxMode,
		MaxAge:   86400,
	})
	writeJSON(w, http.StatusOK, map[string]any{
		"token": token,
		"user":  user,
	})
}

func (s *Server) handleLogout(w http.ResponseWriter, r *http.Request) {
	http.SetCookie(w, &http.Cookie{
		Name:    "dawstream_session",
		Value:   "",
		Path:    "/",
		MaxAge:  -1,
	})
	writeJSON(w, http.StatusOK, map[string]string{"ok": "logged out"})
}

func (s *Server) handleMe(w http.ResponseWriter, r *http.Request) {
	user := auth.FromContext(r.Context())
	if user == nil {
		writeErr(w, http.StatusUnauthorized, "not authenticated")
		return
	}
	writeJSON(w, http.StatusOK, user)
}

// ---- listener page ----------------------------------------------------

func (s *Server) handleListenPage(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("uuid")
	_, ok := s.hub.Get(id)
	if !ok {
		http.NotFound(w, r)
		return
	}
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.Write([]byte(web.ListenPage(id, s.cfg.PublicURL)))
}

// ---- helpers ----------------------------------------------------------

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	json.NewEncoder(w).Encode(v)
}

func writeErr(w http.ResponseWriter, status int, msg string) {
	writeJSON(w, status, map[string]string{"error": msg})
}
