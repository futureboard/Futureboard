package auth

import (
	"context"
	"net/http"
	"strings"
	"time"

	"github.com/google/uuid"
)

type AuthUser struct {
	ID    string
	Email string
	Name  string
}

type contextKey string

const userKey contextKey = "authUser"

// Middleware authenticates a request using JWT bearer token or session cookie.
// In embedded mode, a fixed dev user is injected without real auth.
func Middleware(jwtSecret, mode string) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			user := extractUser(r, jwtSecret, mode)
			ctx := context.WithValue(r.Context(), userKey, user)
			next.ServeHTTP(w, r.WithContext(ctx))
		})
	}
}

func FromContext(ctx context.Context) *AuthUser {
	v, _ := ctx.Value(userKey).(*AuthUser)
	return v
}

func extractUser(r *http.Request, jwtSecret, mode string) *AuthUser {
	// embedded dev mode: any request is treated as the local user
	if mode == "embedded" {
		return &AuthUser{ID: "local", Email: "local@localhost", Name: "Local User"}
	}

	// try Authorization: Bearer <token>
	if auth := r.Header.Get("Authorization"); strings.HasPrefix(auth, "Bearer ") {
		token := strings.TrimPrefix(auth, "Bearer ")
		if c, err := VerifyJWT(token, jwtSecret); err == nil {
			return &AuthUser{ID: c.Sub, Email: c.Email, Name: c.Name}
		}
	}

	// try session cookie
	if cookie, err := r.Cookie("dawstream_session"); err == nil {
		if c, err := VerifyJWT(cookie.Value, jwtSecret); err == nil {
			return &AuthUser{ID: c.Sub, Email: c.Email, Name: c.Name}
		}
	}

	return nil
}

// IssueToken generates a signed JWT for the given user (used by /api/auth/login).
func IssueToken(user AuthUser, secret string, ttl time.Duration) (string, error) {
	now := time.Now()
	return SignJWT(Claims{
		Sub:   user.ID,
		Email: user.Email,
		Name:  user.Name,
		Iat:   now.Unix(),
		Exp:   now.Add(ttl).Unix(),
	}, secret)
}

// NewPublishToken generates a random opaque publish token for a stream session.
func NewPublishToken() string {
	return uuid.New().String() + "-" + uuid.New().String()
}
