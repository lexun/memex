-- Record Aliases for Identity Resolution
-- Adds aliases field to records for canonical identity and disambiguation
-- Part of Entity→Record consolidation

-- Add aliases field to record table
-- Aliases are alternate names/handles used for identity resolution
-- Case-preserving but matched case-insensitively
-- Examples: "Luke" and "@luke" and "lexun" all refer to the same person
DEFINE FIELD aliases ON record TYPE array<string> DEFAULT [];

-- Index for efficient alias lookup
-- Allows searching by alias: SELECT * FROM record WHERE $alias IN aliases
DEFINE INDEX record_aliases_idx ON record FIELDS aliases;
