-- Worker schema for cortex worker persistence
-- Workers are Claude Code processes orchestrated by memex

DEFINE TABLE worker SCHEMAFULL;

-- Worker identity
DEFINE FIELD worker_id ON worker TYPE string;
DEFINE FIELD cwd ON worker TYPE string;
DEFINE FIELD system_prompt ON worker TYPE option<string>;
DEFINE FIELD model ON worker TYPE option<string>;

-- Worker state
DEFINE FIELD state ON worker TYPE string DEFAULT 'ready';
DEFINE FIELD worktree ON worker TYPE option<string>;
DEFINE FIELD current_task ON worker TYPE option<string>;

-- Timestamps
DEFINE FIELD started_at ON worker TYPE datetime;
DEFINE FIELD last_activity ON worker TYPE datetime;
DEFINE FIELD created_at ON worker TYPE datetime DEFAULT time::now();
DEFINE FIELD updated_at ON worker TYPE datetime DEFAULT time::now();

-- Message tracking
DEFINE FIELD messages_sent ON worker TYPE int DEFAULT 0;
DEFINE FIELD messages_received ON worker TYPE int DEFAULT 0;

-- Critical for resuming Claude sessions across daemon restarts
DEFINE FIELD last_session_id ON worker TYPE option<string>;

-- Indexes
DEFINE INDEX worker_id_idx ON worker FIELDS worker_id UNIQUE;
DEFINE INDEX worker_state_idx ON worker FIELDS state;
