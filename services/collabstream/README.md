# DAWStream

Audio streaming server for **Futureboard Studio**.  
Lets a DAW session stream master bus audio to listeners via a simple link.

---

## Modes

| Mode | Use case | Auth to create stream | Auth to listen |
|---|---|---|---|
| `embedded` | LAN / local | None | None |
| `central` | Public internet | JWT required | None |

---

## Quick start — embedded (LAN)

```sh
go run ./cmd/dawstream
```

Default: `http://127.0.0.1:8787`

---

## Quick start — central

```sh
DAWSTREAM_MODE=central \
DAWSTREAM_ADDR=0.0.0.0:8787 \
DAWSTREAM_PUBLIC_URL=https://stream.futureboard.studio \
DAWSTREAM_CODEC=opus \
DAWSTREAM_JWT_SECRET=change-me-secret \
go run ./cmd/dawstream
```

---

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `DAWSTREAM_MODE` | `embedded` | `embedded` or `central` |
| `DAWSTREAM_ADDR` | `127.0.0.1:8787` | Bind address |
| `DAWSTREAM_PUBLIC_URL` | `http://127.0.0.1:8787` | Public-facing base URL |
| `DAWSTREAM_CODEC` | `pcm-f32` | `pcm-f32` or `opus` |
| `DAWSTREAM_AUTH_MODE` | _(empty)_ | `jwt` in central mode |
| `DAWSTREAM_JWT_SECRET` | _(required in central)_ | HMAC-SHA256 JWT secret |
| `DAWSTREAM_MAX_STREAMS_PER_USER` | `3` | Max concurrent streams per user |
| `DAWSTREAM_MAX_LISTENERS_PER_STREAM` | `64` | Max listeners per stream |
| `DAWSTREAM_MAX_FRAME_BYTES` | `65536` | Max binary frame size |
| `DAWSTREAM_LISTENER_BUFFER` | `128` | Per-listener frame buffer depth |
| `DAWSTREAM_STREAM_TTL_MINUTES` | `180` | Offline stream expiry (minutes) |

---

## HTTP API

### `GET /health`
```json
{ "ok": true, "service": "DAWStream", "version": "0.1.0", "mode": "embedded" }
```

### `POST /api/streams`
Create a stream session.  
- Embedded: no auth needed.  
- Central: `Authorization: Bearer <jwt>` required.

**Request:**
```json
{ "title": "My Session", "visibility": "public" }
```

**Response:**
```json
{
  "id": "uuid",
  "title": "My Session",
  "listenUrl": "http://host/listen/uuid",
  "publishUrl": "ws://host/ws/publish/uuid",
  "publishToken": "secret",
  "mode": "embedded"
}
```

### `GET /api/streams/{uuid}`
Stream metadata (public).

### `GET /listen/{uuid}`
Listener HTML page (public, no login).

### `GET /ws/publish/{uuid}?token=<publishToken>`
Publisher WebSocket. Token required.

### `GET /ws/listen/{uuid}`
Listener WebSocket. No auth.

---

## Publisher protocol

**Text frames (JSON):**

```json
{ "type": "stream:start", "sampleRate": 48000, "channels": 2, "codec": "pcm-f32", "frameMs": 20, "title": "Session" }
{ "type": "stream:stop" }
{ "type": "stream:ping" }
```

**Binary frames:** Raw PCM Float32 interleaved stereo audio.

---

## Listener protocol

**Receives text (JSON):**
```json
{ "type": "stream:info", "id": "uuid", "title": "...", "sampleRate": 48000, "channels": 2, "codec": "pcm-f32", "frameMs": 20 }
{ "type": "stream:status", "status": "live", "listeners": 3 }
{ "type": "stream:error", "message": "Stream offline" }
```

**Receives binary:** Audio frames forwarded from publisher.

---

## Build

```sh
go build ./cmd/dawstream
```

---

## Deployment — central

Run behind nginx/caddy with WebSocket proxy and TLS:

```nginx
location / {
    proxy_pass         http://127.0.0.1:8787;
    proxy_http_version 1.1;
    proxy_set_header   Upgrade $http_upgrade;
    proxy_set_header   Connection "upgrade";
    proxy_buffering    off;
    proxy_read_timeout 3600s;
}
```
