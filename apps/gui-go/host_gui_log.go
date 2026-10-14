package main

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
)

// guiLogWriter appends WebView (frontend) log records to the per-day
// un clipboard-gui.json file in the shared log directory. That name matches the
// un clipboard-(gui|daemon|cli).json.<date> layout diaglogs already exports and the retired Tauri host
// wrote, so the GUI's runtime logs — which previously went to Sentry only and were
// invisible offline — land on disk for inspection. Every failure path is dropped
// quietly: the frontend flush is fire-and-forget, and a logging failure must never
// surface as a command error.
type guiLogWriter struct {
	mu   sync.Mutex
	file *os.File
	date string
}

var guiLog = &guiLogWriter{}

// guiLogEntry is one WebView record delivered by the append_gui_logs command. The
// frontend already redacts messages and attributes before sending, so this is written
// to disk verbatim.
type guiLogEntry struct {
	Level      string         `json:"level"`
	Msg        string         `json:"msg"`
	Module     string         `json:"module,omitempty"`
	TraceID    string         `json:"trace_id,omitempty"`
	Attributes map[string]any `json:"attributes,omitempty"`
	Time       string         `json:"time"` // ISO-8601 UTC, produced by the WebView
}

// guiLogLine is the JSONL record written to disk. `role` is the host role (gui); `source`
// distinguishes WebView records from host-side ones.
type guiLogLine struct {
	Role       string         `json:"role"`
	Source     string         `json:"source"`
	Level      string         `json:"level"`
	Msg        string         `json:"msg"`
	Module     string         `json:"module,omitempty"`
	TraceID    string         `json:"trace_id,omitempty"`
	Attributes map[string]any `json:"attributes,omitempty"`
	Time       string         `json:"time"`
}

// append writes the batch as JSONL. A failed write is dropped, never returned.
func (w *guiLogWriter) append(entries []guiLogEntry) {
	if len(entries) == 0 {
		return
	}
	w.mu.Lock()
	defer w.mu.Unlock()
	if err := w.ensureFile(); err != nil {
		return
	}
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	for i := range entries {
		if err := enc.Encode(guiLogLine{
			Role:       "gui",
			Source:     "webview",
			Level:      entries[i].Level,
			Msg:        entries[i].Msg,
			Module:     entries[i].Module,
			TraceID:    entries[i].TraceID,
			Attributes: entries[i].Attributes,
			Time:       entries[i].Time,
		}); err != nil {
			continue
		}
	}
	if buf.Len() == 0 {
		return
	}
	_, _ = w.file.Write(buf.Bytes())
	// Best-effort durability; a failed sync must not error the command.
	_ = w.file.Sync()
}

// ensureFile opens today's file (UTC date), reopening it when the day rolls over. A
// failure leaves file nil so the next call retries from scratch.
func (w *guiLogWriter) ensureFile() error {
	today := time.Now().UTC().Format("2006-01-02")
	if w.file != nil && w.date == today {
		return nil
	}
	if w.file != nil {
		_ = w.file.Close()
		w.file = nil
	}
	dir, ok := apppaths.AppLogDir()
	if !ok {
		return os.ErrNotExist
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	path := filepath.Join(dir, "uniclipboard-gui.json."+today)
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		return err
	}
	w.file = f
	w.date = today
	return nil
}

func init() {
	register(map[string]commandFunc{
		// Fire-and-forget: a malformed or missing batch is dropped (nil, nil), never
		// surfaced, so a logging failure can never reach the frontend error handler.
		"append_gui_logs": func(_ context.Context, _ *HostService, args commandArgs) (any, error) {
			var entries []guiLogEntry
			if err := args.decode("entries", &entries); err != nil {
				return nil, nil
			}
			guiLog.append(entries)
			return nil, nil
		},
	})
}
