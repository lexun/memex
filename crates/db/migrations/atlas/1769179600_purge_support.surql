-- Atlas Purge Support Schema
-- Adds fields to enable complete data purge for sensitive information

-- ========== Event table additions ==========
-- Add source_record to track which record created an event
-- Format: "memo:abc123" or "task:xyz789" or "record:def456"
DEFINE FIELD source_record ON event TYPE option<string>;

-- Index for finding events by source record
DEFINE INDEX event_source_record ON event FIELDS source_record;

-- ========== Fact table additions ==========
-- Add valid_from and valid_until fields if not present (for temporal queries)
-- These are already defined in fact_extraction.surql as optional

-- Add superseded_by and superseded_via if not present
-- These are already defined in fact_extraction.surql as optional

-- ========== Record edge additions ==========
-- Add valid_from/valid_until for temporal tracking (matches records pattern)
DEFINE FIELD valid_from ON record_edge TYPE option<datetime>;
DEFINE FIELD valid_until ON record_edge TYPE option<datetime>;
DEFINE FIELD superseded_by ON record_edge TYPE option<record<record_edge>>;
DEFINE FIELD superseded_via ON record_edge TYPE option<string>;
