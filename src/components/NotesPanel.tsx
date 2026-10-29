import { useState } from "react";
import { ArrowLeft, Plus, StickyNote, Trash2 } from "lucide-react";
import { useNow } from "../hooks/useNow";
import { fmtAgo } from "../lib/format";
import { useStore } from "../store/useStore";
import type { Note } from "../types";

function noteTitle(text: string): string {
  const line = text.split("\n").find((value) => value.trim().length > 0);
  return (line ?? "Untitled").trim().slice(0, 64);
}

function notePreview(text: string): string {
  const lines = text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  if (lines.length <= 1) return "No extra text";
  return lines.slice(1).join(" · ").slice(0, 110);
}

export function NotesPanel() {
  const notes = useStore((s) => s.notes);
  const addNote = useStore((s) => s.addNote);
  const updateNote = useStore((s) => s.updateNote);
  const removeNote = useStore((s) => s.removeNote);
  const now = useNow(30_000);

  const [editing, setEditing] = useState<{
    id: string | null;
    text: string;
  } | null>(null);

  const sorted = [...notes].sort((a, b) => b.updatedAt - a.updatedAt);

  const save = () => {
    if (!editing) return;
    const text = editing.text.trim();
    if (editing.id) {
      if (text) updateNote(editing.id, text);
      else removeNote(editing.id);
    } else if (text) {
      addNote(text);
    }
    setEditing(null);
  };

  const remove = () => {
    if (!editing) return;
    if (editing.id) removeNote(editing.id);
    setEditing(null);
  };

  if (editing) {
    return (
      <div className="notes-panel">
        <div className="notes-toolbar">
          <button
            className="btn-small"
            onClick={() => setEditing(null)}
            title="Back to the note list"
          >
            <ArrowLeft size={11} /> Back
          </button>
          <div className="note-actions">
            {editing.id && (
              <button
                className="icon-btn tiny"
                title="Delete note"
                onClick={remove}
              >
                <Trash2 size={12} />
              </button>
            )}
            <button
              className="btn-small primary"
              onClick={save}
              disabled={!editing.text.trim()}
            >
              Save
            </button>
          </div>
        </div>
        <textarea
          className="note-input"
          value={editing.text}
          autoFocus
          spellCheck={false}
          placeholder="Write anything…"
          onChange={(event) =>
            setEditing((current) =>
              current ? { ...current, text: event.target.value } : current,
            )
          }
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
              event.preventDefault();
              save();
            } else if (event.key === "Escape") {
              // Do not let Escape collapse the whole island while editing.
              event.stopPropagation();
              setEditing(null);
            }
          }}
        />
        <div className="note-hint">Ctrl+Enter to save · Esc to go back</div>
      </div>
    );
  }

  return (
    <div className="notes-panel">
      <div className="notes-toolbar">
        <span className="notes-count">
          {notes.length === 0
            ? "Nothing saved yet"
            : `${notes.length} note${notes.length === 1 ? "" : "s"}`}
        </span>
        <button
          className="btn-small primary"
          onClick={() => setEditing({ id: null, text: "" })}
        >
          <Plus size={11} /> New note
        </button>
      </div>

      {sorted.length === 0 ? (
        <div className="empty">
          <div className="empty-icon">
            <StickyNote size={20} />
          </div>
          <div className="empty-title">No notes yet</div>
          <div className="empty-sub">
            Jot down a command, a link or a thought. Notes stay on this PC.
          </div>
        </div>
      ) : (
        <div className="notes-list">
          {sorted.map((note: Note) => (
            <button
              key={note.id}
              className="note-row"
              onClick={() => setEditing({ id: note.id, text: note.text })}
              title="Open note"
            >
              <div className="note-main">
                <div className="note-title">{noteTitle(note.text)}</div>
                <div className="note-preview">{notePreview(note.text)}</div>
              </div>
              <span className="note-time">{fmtAgo(note.updatedAt, now)}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
