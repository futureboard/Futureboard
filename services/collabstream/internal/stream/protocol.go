package stream

import "encoding/json"

// Inbound control messages (publisher → server)

type MsgType string

const (
	MsgStreamStart MsgType = "stream:start"
	MsgStreamStop  MsgType = "stream:stop"
	MsgStreamPing  MsgType = "stream:ping"
)

type PublisherMsg struct {
	Type       MsgType `json:"type"`
	SampleRate int     `json:"sampleRate,omitempty"`
	Channels   int     `json:"channels,omitempty"`
	Codec      string  `json:"codec,omitempty"`
	FrameMs    int     `json:"frameMs,omitempty"`
	Title      string  `json:"title,omitempty"`
}

// Outbound control messages (server → listener)

type StreamInfoMsg struct {
	Type       MsgType `json:"type"`
	ID         string  `json:"id"`
	Title      string  `json:"title"`
	SampleRate int     `json:"sampleRate"`
	Channels   int     `json:"channels"`
	Codec      string  `json:"codec"`
	FrameMs    int     `json:"frameMs"`
}

type StreamStatusMsg struct {
	Type      MsgType `json:"type"`
	Status    string  `json:"status"`
	Listeners int     `json:"listeners"`
}

type StreamErrorMsg struct {
	Type    MsgType `json:"type"`
	Message string  `json:"message"`
}

func NewInfoMsg(s *Session) []byte {
	b, _ := json.Marshal(StreamInfoMsg{
		Type:       "stream:info",
		ID:         s.ID,
		Title:      s.Title,
		SampleRate: s.Config.SampleRate,
		Channels:   s.Config.Channels,
		Codec:      s.Config.Codec,
		FrameMs:    s.Config.FrameMs,
	})
	return b
}

func NewStatusMsg(status string, listeners int) []byte {
	b, _ := json.Marshal(StreamStatusMsg{
		Type:      "stream:status",
		Status:    status,
		Listeners: listeners,
	})
	return b
}

func NewErrorMsg(msg string) []byte {
	b, _ := json.Marshal(StreamErrorMsg{
		Type:    "stream:error",
		Message: msg,
	})
	return b
}
