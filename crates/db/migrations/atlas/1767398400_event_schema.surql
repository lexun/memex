-- Atlas Event Schema
-- Events are immutable records of things that happened

DEFINE TABLE event SCHEMAFULL;

-- When the event occurred
DEFINE FIELD timestamp ON event TYPE datetime DEFAULT time::now();

-- Event type (e.g., "task.created", "task.updated", "git.commit")
DEFINE FIELD event_type ON event TYPE string;

-- Source attribution (nested object)
DEFINE FIELD source ON event TYPE object;
DEFINE FIELD source.actor ON event TYPE string;
DEFINE FIELD source.authority ON event TYPE option<string>;
DEFINE FIELD source.via ON event TYPE option<string>;

-- Flexible payload for event-specific data
DEFINE FIELD payload ON event FLEXIBLE TYPE object;

-- Indexes for efficient queries
DEFINE INDEX event_timestamp ON event FIELDS timestamp;
DEFINE INDEX event_type ON event FIELDS event_type;
