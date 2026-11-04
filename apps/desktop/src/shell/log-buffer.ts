// The log viewer's buffer: the newest `capacity` lines, oldest first, and how many
// were dropped to keep it there. A ring, so a chatty container costs no copying.

import type { LogEntry } from "./shell-host";

export const LOG_CAPACITY = 5000;

export class LogBuffer {
  private readonly ring: (LogEntry | undefined)[];
  private start = 0;
  private size = 0;
  /** Lines dropped because the buffer was full. */
  dropped = 0;

  constructor(readonly capacity: number = LOG_CAPACITY) {
    this.ring = new Array(capacity);
  }

  push(lines: readonly LogEntry[]): void {
    for (const line of lines) {
      const end = (this.start + this.size) % this.capacity;
      this.ring[end] = line;
      if (this.size < this.capacity) {
        this.size++;
      } else {
        this.start = (this.start + 1) % this.capacity;
        this.dropped++;
      }
    }
  }

  /** The lines, oldest first; only those of `services` when it isn't empty. */
  lines(services: ReadonlySet<string> = new Set()): LogEntry[] {
    const out: LogEntry[] = [];
    for (let i = 0; i < this.size; i++) {
      const line = this.ring[(this.start + i) % this.capacity] as LogEntry;
      if (services.size === 0 || services.has(line.service)) out.push(line);
    }
    return out;
  }

  get length(): number {
    return this.size;
  }
}
