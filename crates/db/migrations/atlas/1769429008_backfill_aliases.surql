-- Backfill aliases field for existing records
-- Fixes bug where records created before the aliases migration have NULL aliases
-- which causes schema validation errors on UPDATE operations

-- Set aliases to empty array for any record where it's currently NONE/NULL
UPDATE record SET aliases = [] WHERE aliases IS NONE;
