# Mochi DAW Server

Prototype Bun API for project metadata and audio-file storage. This is intentionally small for v0.1: SQLite stores project/file metadata, while imported WAV/MP3 files are written to local object-storage-like folders under `uploads/`.

## Run

```bash
bun install
bun run dev
```

Default URL:

```bash
http://localhost:3001
```

## Environment

```bash
PORT=3001
CORS_ORIGIN=http://localhost:5173
UPLOADS_DIR=./uploads
MAX_AUDIO_FILE_BYTES=262144000
```

## Endpoints

```txt
GET    /health
GET    /api/projects
POST   /api/projects
GET    /api/projects/:id
PUT    /api/projects/:id
DELETE /api/projects/:id
POST   /api/projects/:id/save
POST   /api/projects/:id/export       # 501 placeholder

GET    /api/projects/:projectId/files
POST   /api/projects/:projectId/files  # multipart field: file
GET    /api/projects/:projectId/files/:fileId
GET    /api/projects/:projectId/files/:fileId?metadata=true
```

The server validates uploads to the v0.1 scope: WAV and MP3 only. Browser-side decoding and waveform generation remain in the web app.
