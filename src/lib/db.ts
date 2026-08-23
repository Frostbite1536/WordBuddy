import Database from "@tauri-apps/plugin-sql";

// Memoize the in-flight load+migrate promise so concurrent first
// callers (sweep + History page + ChatBar effect on cold start) don't
// each invoke `Database.load` and re-run the schema bootstrap. Without
// this, parallel `CREATE INDEX IF NOT EXISTS` calls intermittently
// surface as `database is locked` on slow disks (Defender scanning).
// On bootstrap failure the promise is cleared so the next caller can
// retry instead of being permanently stuck.
let dbPromise: Promise<Database> | null = null;

export function getDb(): Promise<Database> {
  if (!dbPromise) {
    dbPromise = (async () => {
      const d = await Database.load("sqlite:wordbuddy.db");
      await initSchema(d);
      return d;
    })().catch((err) => {
      dbPromise = null;
      throw err;
    });
  }
  return dbPromise;
}

// Versioned migration framework (O3 audit). Each migration is an
// async function that runs exactly once per install. The current
// schema version is stored in SQLite's PRAGMA user_version; on each
// app start we walk forward from the stored version, running each
// migration in order. Adding a column?  Append a new migration
// (don't edit existing ones) and bump SCHEMA_VERSION. Migrations
// must be idempotent (use CREATE … IF NOT EXISTS / ALTER … failure
// recovery) so a partially-applied migration on a crash mid-startup
// can re-run cleanly on the next launch.
const SCHEMA_VERSION = 1;

type Migration = (db: Database) => Promise<void>;

const MIGRATIONS: Migration[] = [
  // v0 -> v1: initial schema. Captures the full table set as it
  // existed when the migration framework landed; future bumps are
  // additive ALTERs in their own migration entry.
  async (db) => {
    await db.execute(`
      CREATE TABLE IF NOT EXISTS conversations (
        id TEXT PRIMARY KEY,
        created_at INTEGER NOT NULL,
        program TEXT,
        module_id TEXT
      )
    `);
    await db.execute(`
      CREATE TABLE IF NOT EXISTS messages (
        id TEXT PRIMARY KEY,
        conversation_id TEXT NOT NULL REFERENCES conversations(id),
        role TEXT NOT NULL,
        content TEXT NOT NULL,
        timestamp INTEGER NOT NULL
      )
    `);
  },
];

async function getUserVersion(db: Database): Promise<number> {
  const rows = await db.select<{ user_version: number }[]>(
    "PRAGMA user_version",
  );
  return Number(rows[0]?.user_version ?? 0);
}

async function setUserVersion(db: Database, v: number): Promise<void> {
  // PRAGMA does not accept bound parameters. v is an integer we
  // generated, not user input, so string interpolation is safe.
  await db.execute(`PRAGMA user_version = ${Number(v)}`);
}

async function initSchema(db: Database): Promise<void> {
  // Migration walk. Each step runs in order from `current+1` up to
  // SCHEMA_VERSION; a failed step throws and aborts startup with the
  // user_version unchanged so the next launch retries from the same
  // point. Running the failed migration twice on the next launch is
  // safe because every DDL uses IF NOT EXISTS.
  const current = await getUserVersion(db);
  if (current > SCHEMA_VERSION) {
    // The DB was written by a newer build than this binary. Refuse
    // to downgrade — partial column reads against a missing column
    // would corrupt the user's data.
    throw new Error(
      `wordbuddy.db user_version=${current} is newer than this build's SCHEMA_VERSION=${SCHEMA_VERSION}. Downgrade not supported.`,
    );
  }
  for (let v = current; v < SCHEMA_VERSION; v++) {
    await MIGRATIONS[v](db);
    await setUserVersion(db, v + 1);
  }
}

// (The full DDL for the initial schema lives in the v0→v1 migration
// above. Add a new migration entry to MIGRATIONS for any future
// schema change — never edit the v0→v1 entry.)

export interface ConversationRow {
  id: string;
  created_at: number;
  program: string | null;
  module_id: string | null;
}

export interface MessageRow {
  id: string;
  conversation_id: string;
  role: string;
  content: string;
  timestamp: number;
}

// Audit M4: tauri-plugin-sql hands each execute a connection from a
// sqlx pool (default max_connections=10) — BEGIN/COMMIT issued as
// separate execute calls are NOT pinned to one connection. Interleaved
// writers could land statements outside the transaction, silently
// voiding its atomicity. Serializing every write through this promise
// chain makes overlap impossible at the only layer we control; with no
// concurrent acquirer, sqlx's LIFO idle reuse keeps one transaction on
// one connection in practice.
let writeChain: Promise<void> = Promise.resolve();
function withWriteLock<T>(fn_: () => Promise<T>): Promise<T> {
  const run = writeChain.then(fn_, fn_);
  writeChain = run.then(
    () => undefined,
    () => undefined,
  );
  return run;
}

export async function saveConversation(
  conversationId: string,
  program: string | null,
  moduleId: string | null,
): Promise<void> {
  return withWriteLock(async () => {
    const d = await getDb();
    await d.execute(
      `INSERT OR IGNORE INTO conversations (id, created_at, program, module_id)
       VALUES ($1, $2, $3, $4)`,
      [conversationId, Date.now(), program ?? null, moduleId ?? null],
    );
  });
}

export async function saveMessage(
  messageId: string,
  conversationId: string,
  role: string,
  content: string,
  timestamp: number,
): Promise<void> {
  return withWriteLock(async () => {
    const d = await getDb();
    // Do NOT store screenshots — only text content (INV-SEC-004)
    await d.execute(
      `INSERT OR REPLACE INTO messages (id, conversation_id, role, content, timestamp)
       VALUES ($1, $2, $3, $4, $5)`,
      [messageId, conversationId, role, content, timestamp],
    );
  });
}

export interface MessageWrite {
  id: string;
  role: string;
  content: string;
  timestamp: number;
}

// Atomic post-stream persistence: write the conversation row plus
// every message that belongs to this turn under a single transaction
// so a partial failure doesn't strand a conversation row with
// missing messages (the History page would render ghost
// conversations otherwise). On any error the whole turn rolls back
// and the caller learns about it via the thrown exception.
export async function saveTurn(
  conversationId: string,
  program: string | null,
  moduleId: string | null,
  messages: MessageWrite[],
): Promise<void> {
  return withWriteLock(async () => {
    const d = await getDb();
    await d.execute("BEGIN");
    try {
      await d.execute(
        `INSERT OR IGNORE INTO conversations (id, created_at, program, module_id)
         VALUES ($1, $2, $3, $4)`,
        [conversationId, Date.now(), program ?? null, moduleId ?? null],
      );
      for (const m of messages) {
        // INV-SEC-004 — text content only.
        await d.execute(
          `INSERT OR REPLACE INTO messages (id, conversation_id, role, content, timestamp)
           VALUES ($1, $2, $3, $4, $5)`,
          [m.id, conversationId, m.role, m.content, m.timestamp],
        );
      }
      await d.execute("COMMIT");
    } catch (err) {
      // Best-effort rollback. If even the rollback fails we still want
      // the original error to propagate — a stuck transaction will be
      // cleared by the next session's connection reset.
      try {
        await d.execute("ROLLBACK");
      } catch {
        // Swallowed intentionally — the original error is already on
        // its way up.
      }
      throw err;
    }
  });
}

export async function loadConversations(): Promise<ConversationRow[]> {
  const d = await getDb();
  return d.select<ConversationRow[]>(
    "SELECT * FROM conversations ORDER BY created_at DESC LIMIT 100",
  );
}

export async function loadMessages(
  conversationId: string,
): Promise<MessageRow[]> {
  const d = await getDb();
  return d.select<MessageRow[]>(
    "SELECT * FROM messages WHERE conversation_id = $1 ORDER BY timestamp ASC",
    [conversationId],
  );
}

export async function deleteConversation(
  conversationId: string,
): Promise<void> {
  return withWriteLock(async () => {
    const d = await getDb();
    await d.execute("DELETE FROM messages WHERE conversation_id = $1", [
      conversationId,
    ]);
    await d.execute("DELETE FROM conversations WHERE id = $1", [conversationId]);
  });
}
