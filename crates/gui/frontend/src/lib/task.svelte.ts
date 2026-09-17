import { Channel, invoke } from "@tauri-apps/api/core";

export type TaskKind = "backup" | "verify" | "restore";

export type Phase =
  | "opening"
  | "scanning"
  | "hashing"
  | "packing"
  | "verifying"
  | "verifyingContent"
  | "extracting";

type TaskEvent =
  | {
      kind: "progress";
      phase: Phase;
      done: number;
      total: number;
      item: string | null;
      itemDone: number;
      itemTotal: number;
    }
  | { kind: "log"; message: string };

export type Progress = Omit<Extract<TaskEvent, { kind: "progress" }>, "kind">;
export type LogEntry = { id: number; time: number; text: string; level: "info" | "success" | "error" };
export type TaskStatus = "running" | "done" | "failed" | "cancelled";

export const phaseTitles: Record<Phase, string> = {
  opening: "Opening archive…",
  scanning: "Scanning files…",
  hashing: "Checking file contents…",
  packing: "Compressing and writing…",
  verifying: "Verifying archive data…",
  verifyingContent: "Checking stored file contents…",
  extracting: "Restoring files…",
};

export const taskSteps: Record<TaskKind, { phases: Phase[]; label: string }[]> = {
  backup: [
    { phases: ["opening", "scanning"], label: "Scan files" },
    { phases: ["hashing"], label: "Check contents" },
    { phases: ["packing"], label: "Compress & write" },
  ],
  verify: [
    { phases: ["opening"], label: "Open archive" },
    { phases: ["verifying"], label: "Verify data" },
    { phases: ["verifyingContent"], label: "Check contents" },
  ],
  restore: [
    { phases: ["opening"], label: "Open archive" },
    { phases: ["extracting"], label: "Restore files" },
  ],
};

const taskNames: Record<TaskKind, string> = {
  backup: "Backup",
  verify: "Verification",
  restore: "Restore",
};

/** Tracks the single long-running operation: progress, log and cancellation. */
class TaskRunner {
  /** The task whose progress and log are shown (running or last finished). */
  kind = $state<TaskKind | null>(null);
  status = $state<TaskStatus | null>(null);
  progress = $state<Progress | null>(null);
  log = $state<LogEntry[]>([]);
  startedAt = $state(0);
  endedAt = $state(0);
  phaseStartedAt = $state(0);
  phaseStartDone = $state(0);
  cancelling = $state(false);
  now = $state(0);

  #logId = 0;
  #timer: ReturnType<typeof setInterval> | undefined;

  get running() {
    return this.status === "running";
  }

  get elapsed() {
    return (this.running ? this.now : this.endedAt) - this.startedAt;
  }

  /** Bytes per second in the current phase, once it has run for a moment. */
  get rate() {
    const p = this.progress;
    const secs = (this.now - this.phaseStartedAt) / 1000;
    if (!p || p.phase === "scanning" || secs < 1) return null;
    return (p.done - this.phaseStartDone) / secs;
  }

  /** Estimated seconds left in the current phase. */
  get eta() {
    const p = this.progress;
    const rate = this.rate;
    if (!p || !rate || p.total <= 0) return null;
    return Math.max(0, (p.total - p.done) / rate);
  }

  #push(text: string, level: LogEntry["level"] = "info") {
    this.log.push({ id: ++this.#logId, time: Date.now(), text, level });
  }

  async run<T>(kind: TaskKind, command: string, args: Record<string, unknown>): Promise<T | undefined> {
    if (this.running) return undefined;
    const onEvent = new Channel<TaskEvent>();
    onEvent.onmessage = (event) => {
      if (event.kind === "log") {
        this.#push(event.message);
        return;
      }
      const { kind: _, ...p } = event;
      if (this.progress?.phase !== p.phase) {
        this.phaseStartedAt = Date.now();
        this.phaseStartDone = p.done;
      }
      this.progress = p;
    };

    this.kind = kind;
    this.status = "running";
    this.progress = null;
    this.log = [];
    this.cancelling = false;
    this.startedAt = this.now = this.phaseStartedAt = Date.now();
    this.#timer = setInterval(() => (this.now = Date.now()), 250);

    try {
      const result = await invoke<T>(command, { ...args, onEvent });
      this.status = "done";
      this.#push(`${taskNames[kind]} finished in ${formatDuration(Date.now() - this.startedAt)}`, "success");
      return result;
    } catch (e) {
      if (e === "cancelled") {
        this.status = "cancelled";
        this.#push(`${taskNames[kind]} cancelled`, "error");
      } else {
        this.status = "failed";
        this.#push(`${taskNames[kind]} failed: ${e}`, "error");
      }
      return undefined;
    } finally {
      clearInterval(this.#timer);
      this.endedAt = this.now = Date.now();
      this.cancelling = false;
    }
  }

  async cancel() {
    if (!this.running || this.cancelling) return;
    this.cancelling = true;
    this.#push("Cancelling…");
    await invoke("cancel_task");
  }

  logText() {
    return this.log.map((l) => `${formatClock(l.time)}  ${l.text}`).join("\n");
  }
}

export const task = new TaskRunner();

export function humanBytes(n: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = n;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export function formatDuration(ms: number): string {
  const secs = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = String(secs % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

export function formatClock(time: number): string {
  return new Date(time).toLocaleTimeString([], { hour12: false });
}
