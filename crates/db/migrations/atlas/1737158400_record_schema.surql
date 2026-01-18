-- Atlas Record Schema
-- Records are typed, stateful objects with flexible edges for graph relationships
-- Part of the three-tier model: Events (immutable) → Records (stateful) → Graph (index)

-- ========== Record table ==========
DEFINE TABLE record SCHEMAFULL;

-- Core identity
DEFINE FIELD record_type ON record TYPE string;  -- repo, team, person, company, initiative, rule, skill, document, task
DEFINE FIELD name ON record TYPE string;
DEFINE FIELD description ON record TYPE option<string>;

-- Flexible content for type-specific fields
-- Examples:
--   repo: { path, default_branch, languages }
--   person: { email, role, preferences }
--   rule: { content, severity, auto_apply }
--   task: { status, priority, project }
DEFINE FIELD content ON record FLEXIBLE TYPE object;

-- Soft delete support (records are never hard-deleted)
DEFINE FIELD deleted_at ON record TYPE option<datetime>;
DEFINE FIELD superseded_by ON record TYPE option<record<record>>;
DEFINE FIELD superseded_via ON record TYPE option<string>;  -- event ID that caused supersession

-- Timestamps
DEFINE FIELD created_at ON record TYPE datetime DEFAULT time::now();
DEFINE FIELD updated_at ON record TYPE datetime DEFAULT time::now();

-- Indexes
DEFINE INDEX record_type_idx ON record FIELDS record_type;
DEFINE INDEX record_name_idx ON record FIELDS name;
DEFINE INDEX record_type_name_idx ON record FIELDS record_type, name UNIQUE;
DEFINE INDEX record_deleted_idx ON record FIELDS deleted_at;

-- Full-text search on name and description
DEFINE ANALYZER record_analyzer TOKENIZERS class FILTERS lowercase, snowball(english);
DEFINE INDEX record_name_search ON record FIELDS name SEARCH ANALYZER record_analyzer BM25;
DEFINE INDEX record_desc_search ON record FIELDS description SEARCH ANALYZER record_analyzer BM25;

-- ========== Record Edge table ==========
-- Edges connect records with typed relationships
-- This enables flexible graph structures (a rule can apply_to both a repo AND a team)
DEFINE TABLE record_edge SCHEMAFULL;

DEFINE FIELD source ON record_edge TYPE record<record>;
DEFINE FIELD target ON record_edge TYPE record<record>;

-- Relationship types:
--   applies_to: rule applies to repo/team/person
--   belongs_to: repo belongs to team, team belongs to company
--   member_of: person is member of team
--   owns: person/team owns repo/initiative
--   available_to: skill available to person/team/repo
--   depends_on: task depends on task
--   related_to: generic association
DEFINE FIELD relation ON record_edge TYPE string;

-- Optional metadata for the edge
DEFINE FIELD metadata ON record_edge FLEXIBLE TYPE option<object>;

DEFINE FIELD created_at ON record_edge TYPE datetime DEFAULT time::now();

-- Indexes for efficient traversal
DEFINE INDEX edge_source_idx ON record_edge FIELDS source;
DEFINE INDEX edge_target_idx ON record_edge FIELDS target;
DEFINE INDEX edge_relation_idx ON record_edge FIELDS relation;
DEFINE INDEX edge_source_relation_idx ON record_edge FIELDS source, relation;
DEFINE INDEX edge_target_relation_idx ON record_edge FIELDS target, relation;

-- Unique constraint: only one edge of each type between two records
DEFINE INDEX edge_unique_idx ON record_edge FIELDS source, target, relation UNIQUE;
