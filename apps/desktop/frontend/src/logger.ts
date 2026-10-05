export type LogLevel = 'info' | 'warn' | 'error' | 'success' | 'ipc';

export interface LogEntry {
  id: string;
  timestamp: string;
  level: LogLevel;
  source: string;
  message: string;
  data?: unknown;
}

export type LogListener = (entry: LogEntry) => void;

class LogStore {
  private entries: LogEntry[] = [];
  private listeners: Set<LogListener> = new Set();
  private maxEntries: number = 200;

  constructor() {
    this.info('SYSTEM', 'Planck Optical Light-Table OS initialized');
  }

  public subscribe(listener: LogListener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  public getEntries(): LogEntry[] {
    return [...this.entries];
  }

  public clear(): void {
    this.entries = [];
    this.notify({
      id: this.nextId(),
      timestamp: this.formatTime(),
      level: 'info',
      source: 'SYSTEM',
      message: 'Diagnostic log cleared',
    });
  }

  public log(level: LogLevel, source: string, message: string, data?: unknown): void {
    const entry: LogEntry = {
      id: this.nextId(),
      timestamp: this.formatTime(),
      level,
      source: source.toUpperCase(),
      message,
      data,
    };
    this.entries.push(entry);
    if (this.entries.length > this.maxEntries) {
      this.entries.shift();
    }
    this.notify(entry);
  }

  public info(source: string, message: string, data?: unknown): void {
    this.log('info', source, message, data);
  }

  public ipc(source: string, message: string, data?: unknown): void {
    this.log('ipc', source, message, data);
  }

  public warn(source: string, message: string, data?: unknown): void {
    this.log('warn', source, message, data);
  }

  public error(source: string, message: string, data?: unknown): void {
    this.log('error', source, message, data);
  }

  public success(source: string, message: string, data?: unknown): void {
    this.log('success', source, message, data);
  }

  private nextId(): string {
    return `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
  }

  private formatTime(): string {
    const d = new Date();
    const pad = (n: number) => n.toString().padStart(2, '0');
    const ms = d.getMilliseconds().toString().padStart(3, '0');
    return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${ms}`;
  }

  private notify(entry: LogEntry): void {
    for (const listener of this.listeners) {
      try {
        listener(entry);
      } catch {}
    }
  }
}

export const logger = new LogStore();
