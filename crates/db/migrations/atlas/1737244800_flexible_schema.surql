-- Flexible Record Schema Migration
-- This migration introduces the two-dimensional type system: kind (functional) + type (semantic)
--
-- The existing record_type field is preserved for backward compatibility.
-- New fields `kind` and `semantic_type` are added to enable the flexible schema.
--
-- Migration strategy:
-- 1. Add new fields with defaults
-- 2. Backfill existing records based on record_type
-- 3. (Future) Deprecate record_type in favor of kind + semantic_type

-- ========== Add new fields to record table ==========

-- Record kind (functional dimension): how the record behaves
-- Values: document, task, rule, skill, goal, message, thread, entry, config
DEFINE FIELD kind ON record TYPE string DEFAULT "document";

-- Semantic type (domain dimension): what the record represents
-- Values: any string (person, company, team, repo, technology, project, etc.)
-- User-defined types are allowed for extensibility
DEFINE FIELD semantic_type ON record TYPE string DEFAULT "generic";

-- Add indexes for the new fields
DEFINE INDEX record_kind_idx ON record FIELDS kind;
DEFINE INDEX record_semantic_type_idx ON record FIELDS semantic_type;
DEFINE INDEX record_kind_semantic_idx ON record FIELDS kind, semantic_type;

-- ========== Type Schema table (optional schema definitions) ==========
-- Type schemas define expected structure for record content
-- They're stored as records themselves for flexibility

DEFINE TABLE type_schema SCHEMAFULL;

-- Which semantic type this schema defines
DEFINE FIELD for_type ON type_schema TYPE string;

-- Which kinds this schema applies to (empty array = all kinds)
DEFINE FIELD for_kinds ON type_schema TYPE array<string> DEFAULT [];

-- Display metadata for UI
DEFINE FIELD display_name ON type_schema TYPE option<string>;
DEFINE FIELD description ON type_schema TYPE option<string>;
DEFINE FIELD icon ON type_schema TYPE option<string>;
DEFINE FIELD color ON type_schema TYPE option<string>;

-- Field definitions (JSON array of field specs)
-- Each field: { name, label, field_type, required, default, description, ref_types }
DEFINE FIELD fields ON type_schema FLEXIBLE TYPE array DEFAULT [];

-- Timestamps
DEFINE FIELD created_at ON type_schema TYPE datetime DEFAULT time::now();
DEFINE FIELD updated_at ON type_schema TYPE datetime DEFAULT time::now();

-- Unique constraint: one schema per type
DEFINE INDEX type_schema_type_idx ON type_schema FIELDS for_type UNIQUE;

-- ========== Backfill existing records ==========
-- Map existing record_type values to (kind, semantic_type) pairs

-- Workflow types -> specific kind, generic semantic type
UPDATE record SET kind = "task", semantic_type = "generic" WHERE record_type = "task";
UPDATE record SET kind = "goal", semantic_type = "generic" WHERE record_type = "goal";
UPDATE record SET kind = "rule", semantic_type = "generic" WHERE record_type = "rule";
UPDATE record SET kind = "skill", semantic_type = "generic" WHERE record_type = "skill";
UPDATE record SET kind = "message", semantic_type = "generic" WHERE record_type = "message";
UPDATE record SET kind = "thread", semantic_type = "generic" WHERE record_type = "thread";
UPDATE record SET kind = "entry", semantic_type = "generic" WHERE record_type = "entry";

-- Config types
UPDATE record SET kind = "config", semantic_type = "mcp_server" WHERE record_type = "mcp_server";

-- Entity types -> document kind, specific semantic type
UPDATE record SET kind = "document", semantic_type = "repo" WHERE record_type = "repo";
UPDATE record SET kind = "document", semantic_type = "team" WHERE record_type = "team";
UPDATE record SET kind = "document", semantic_type = "person" WHERE record_type = "person";
UPDATE record SET kind = "document", semantic_type = "company" WHERE record_type = "company";
UPDATE record SET kind = "document", semantic_type = "project" WHERE record_type = "initiative";
UPDATE record SET kind = "document", semantic_type = "technology" WHERE record_type = "technology";
UPDATE record SET kind = "document", semantic_type = "generic" WHERE record_type = "document";
UPDATE record SET kind = "document", semantic_type = "project" WHERE record_type = "project";

-- ========== Insert built-in type schemas ==========

-- Person schema
CREATE type_schema SET
    for_type = "person",
    for_kinds = ["document"],
    display_name = "Person",
    description = "A person (developer, user, stakeholder)",
    fields = [
        { name: "email", field_type: "email" },
        { name: "role", field_type: "string" },
        { name: "github", field_type: "string" },
        { name: "team", field_type: "record_ref" }
    ];

-- Repository schema
CREATE type_schema SET
    for_type = "repo",
    for_kinds = ["document"],
    display_name = "Repository",
    description = "A code repository",
    fields = [
        { name: "path", field_type: "string", required: true },
        { name: "default_branch", field_type: "string", default: "main" },
        { name: "languages", field_type: "array" },
        { name: "remote_url", field_type: "url" }
    ];

-- Technology schema
CREATE type_schema SET
    for_type = "technology",
    for_kinds = ["document"],
    display_name = "Technology",
    description = "A technology, tool, framework, or service",
    fields = [
        { name: "category", field_type: "string" },
        { name: "version", field_type: "string" },
        { name: "website", field_type: "url" },
        { name: "docs_url", field_type: "url" }
    ];

-- MCP Server schema
CREATE type_schema SET
    for_type = "mcp_server",
    for_kinds = ["config"],
    display_name = "MCP Server",
    description = "Model Context Protocol server configuration",
    fields = [
        { name: "command", field_type: "string", required: true },
        { name: "args", field_type: "array" },
        { name: "env", field_type: "object" }
    ];

-- Team schema
CREATE type_schema SET
    for_type = "team",
    for_kinds = ["document"],
    display_name = "Team",
    description = "A team of people",
    fields = [
        { name: "organization", field_type: "record_ref" },
        { name: "lead", field_type: "record_ref" }
    ];

-- Project schema
CREATE type_schema SET
    for_type = "project",
    for_kinds = ["document"],
    display_name = "Project",
    description = "A project or initiative",
    fields = [
        { name: "repo_url", field_type: "url" },
        { name: "tools", field_type: "array" },
        { name: "documents", field_type: "array" }
    ];
