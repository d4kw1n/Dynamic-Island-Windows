import { useState, type FormEvent } from "react";
import { ListChecks, Plus, Trash2 } from "lucide-react";
import { useStore } from "../store/useStore";
import { CheckBubble } from "./Bits";

export function TasksPanel() {
  const tasks = useStore((s) => s.tasks);
  const addTask = useStore((s) => s.addTask);
  const toggleTask = useStore((s) => s.toggleTask);
  const removeTask = useStore((s) => s.removeTask);
  const clearDone = useStore((s) => s.clearDone);

  const [text, setText] = useState("");

  const open = tasks.filter((task) => !task.done);
  const done = tasks.filter((task) => task.done);
  const ordered = [...open, ...done];

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!text.trim()) return;
    addTask(text);
    setText("");
  };

  return (
    <div className="tasks-panel">
      <form className="task-add" onSubmit={submit}>
        <input
          value={text}
          onChange={(event) => setText(event.target.value)}
          placeholder="Add a task and press Enter"
          aria-label="New task"
          spellCheck={false}
        />
        <button type="submit" title="Add task" disabled={!text.trim()}>
          <Plus size={14} />
        </button>
      </form>

      {ordered.length === 0 ? (
        <div className="empty">
          <div className="empty-icon">
            <ListChecks size={20} />
          </div>
          <div className="empty-title">No tasks yet</div>
          <div className="empty-sub">
            Add one above. Keep it small enough to finish in a single sitting.
          </div>
        </div>
      ) : (
        <div className="task-list">
          {ordered.map((task) => (
            <div key={task.id} className={`task ${task.done ? "done" : ""}`}>
              <button
                className="task-toggle"
                onClick={() => toggleTask(task.id)}
                aria-label={task.done ? "Mark as open" : "Mark as done"}
              >
                <CheckBubble done={task.done} />
              </button>
              <span className="task-text">{task.text}</span>
              <button
                className="task-del"
                onClick={() => removeTask(task.id)}
                aria-label="Delete task"
              >
                <Trash2 size={12} />
              </button>
            </div>
          ))}
        </div>
      )}

      <div className="tasks-foot">
        <span>
          {open.length} open
          {done.length > 0 ? ` · ${done.length} done` : ""}
        </span>
        {done.length > 0 && (
          <button className="link-btn" onClick={clearDone}>
            Clear done
          </button>
        )}
      </div>
    </div>
  );
}
