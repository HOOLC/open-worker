import { randomUUID } from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import path from "node:path";

import { ensureDir } from "../utils/fs.js";

export const SPOOL_DATABASE_FILENAME = "spool.sqlite";
export const SPOOL_SCHEMA_VERSION = 1;
const SPOOL_BUSY_TIMEOUT_MS = 5_000;

export interface PersistedSpoolRow {
  readonly id: string;
  readonly direction: "inbound" | "outbound";
  readonly channel: string;
  readonly payload: string;
  readonly receiveCount: number;
  readonly createdAt: string;
}

export class SpoolStore {
  readonly #stateDir: string;
  #database: DatabaseSync | undefined;
  #loaded = false;

  constructor(stateDir: string) {
    this.#stateDir = stateDir;
  }

  async load(): Promise<void> {
    await ensureDir(this.#stateDir);
    if (this.#loaded) {
      return;
    }
    this.#database = new DatabaseSync(path.join(this.#stateDir, SPOOL_DATABASE_FILENAME));
    this.#database.exec(`
      PRAGMA foreign_keys = ON;
      PRAGMA journal_mode = WAL;
      PRAGMA synchronous = NORMAL;
      PRAGMA busy_timeout = ${SPOOL_BUSY_TIMEOUT_MS};
    `);
    this.#migrate();
    this.#loaded = true;
  }

  claim(options: { readonly direction: "inbound" | "outbound"; readonly owner: string; readonly leaseMs: number }): PersistedSpoolRow[] {
    const now = new Date().toISOString();
    const leaseUntil = new Date(Date.now() + options.leaseMs).toISOString();
    return this.#transaction(() => {
      this.#databaseRequired()
        .prepare(
          `UPDATE spool
           SET locked_by = ?, locked_at = ?, lease_until = ?, receive_count = receive_count + 1
           WHERE id IN (
             SELECT id FROM spool
             WHERE direction = ?
               AND acked_at IS NULL
               AND (locked_by IS NULL OR lease_until <= ?)
           )`,
        )
        .run(options.owner, now, leaseUntil, options.direction, now);
      return this.#databaseRequired()
        .prepare("SELECT id, direction, channel, payload, receive_count, created_at FROM spool WHERE direction = ? AND locked_by = ? AND acked_at IS NULL")
        .all(options.direction, options.owner)
        .map((row) => this.#rowToSpool(row as Record<string, unknown>));
    });
  }

  enqueue(options: { readonly direction: "inbound" | "outbound"; readonly channel: string; readonly payload: unknown; readonly id?: string | undefined }): string {
    const id = options.id?.trim() || randomUUID();
    this.#transaction(() => {
      this.#databaseRequired()
        .prepare("INSERT INTO spool (id, direction, channel, payload, created_at) VALUES (?, ?, ?, ?, ?)")
        .run(id, options.direction, options.channel, JSON.stringify(options.payload), new Date().toISOString());
    });
    return id;
  }

  ack(id: string): void {
    this.#transaction(() => {
      this.#databaseRequired()
        .prepare("UPDATE spool SET acked_at = ?, locked_by = NULL, lease_until = NULL WHERE id = ?")
        .run(new Date().toISOString(), id);
    });
  }

  close(): void {
    this.#database?.close();
    this.#database = undefined;
    this.#loaded = false;
  }

  #migrate(): void {
    const database = this.#databaseRequired();
    database.exec(`
      CREATE TABLE IF NOT EXISTS schema_migrations (
        version INTEGER PRIMARY KEY,
        name TEXT NOT NULL DEFAULT '',
        applied_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS spool (
        id TEXT PRIMARY KEY,
        direction TEXT NOT NULL,
        channel TEXT NOT NULL,
        payload TEXT NOT NULL,
        locked_by TEXT,
        locked_at TEXT,
        lease_until TEXT,
        receive_count INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL,
        acked_at TEXT
      );
      CREATE INDEX IF NOT EXISTS idx_spool_claim
        ON spool (direction, acked_at, lease_until, created_at);
      CREATE TABLE IF NOT EXISTS process_lease (
        role TEXT PRIMARY KEY,
        owner_id TEXT NOT NULL,
        lease_until TEXT NOT NULL,
        updated_at TEXT NOT NULL
      );
    `);
    const applied = database.prepare("SELECT version FROM schema_migrations WHERE version = ?").get(SPOOL_SCHEMA_VERSION) as { version?: number } | undefined;
    if (!applied) {
      database.prepare("INSERT INTO schema_migrations (version, name, applied_at) VALUES (?, ?, ?)").run(SPOOL_SCHEMA_VERSION, "spool_queue", new Date().toISOString());
    }
  }

  #transaction<T>(operation: () => T): T {
    const database = this.#databaseRequired();
    database.exec("BEGIN IMMEDIATE");
    try {
      const result = operation();
      database.exec("COMMIT");
      return result;
    } catch (error) {
      database.exec("ROLLBACK");
      throw error;
    }
  }

  #databaseRequired(): DatabaseSync {
    if (!this.#database) {
      throw new Error("SpoolStore has not been loaded");
    }
    return this.#database;
  }

  #rowToSpool(row: Record<string, unknown>): PersistedSpoolRow {
    return {
      id: String(row.id),
      direction: String(row.direction) as PersistedSpoolRow["direction"],
      channel: String(row.channel),
      payload: String(row.payload),
      receiveCount: Number(row.receive_count ?? 0),
      createdAt: String(row.created_at),
    };
  }
}
