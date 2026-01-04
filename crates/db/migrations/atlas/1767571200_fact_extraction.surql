-- Atlas Fact Extraction Schema
-- Facts are medium-granularity assertions extracted from episodes
-- Entities are named things referenced by facts

-- ========== Fact table ==========
DEFINE TABLE fact SCHEMAFULL;

-- Core content
DEFINE FIELD content ON fact TYPE string;
DEFINE FIELD fact_type ON fact TYPE string DEFAULT "statement";
DEFINE FIELD confidence ON fact TYPE float DEFAULT 1.0;

-- Context boundary
DEFINE FIELD project ON fact TYPE option<string>;

-- Provenance: which episodes this fact came from
DEFINE FIELD source_episodes ON fact TYPE array DEFAULT [];
DEFINE FIELD source_episodes.* ON fact TYPE object;
DEFINE FIELD source_episodes.*.episode_type ON fact TYPE string;
DEFINE FIELD source_episodes.*.episode_id ON fact TYPE string;

-- Timestamps
DEFINE FIELD created_at ON fact TYPE datetime DEFAULT time::now();
DEFINE FIELD updated_at ON fact TYPE datetime DEFAULT time::now();

-- Usage tracking
DEFINE FIELD access_count ON fact TYPE int DEFAULT 0;
DEFINE FIELD last_accessed ON fact TYPE option<datetime>;

-- Indexes for queries
DEFINE INDEX fact_project ON fact FIELDS project;
DEFINE INDEX fact_type ON fact FIELDS fact_type;
DEFINE INDEX fact_confidence ON fact FIELDS confidence;
DEFINE INDEX fact_created ON fact FIELDS created_at;

-- Full-text search on fact content
DEFINE ANALYZER fact_analyzer TOKENIZERS class FILTERS lowercase, snowball(english);
DEFINE INDEX fact_content_search ON fact FIELDS content SEARCH ANALYZER fact_analyzer BM25;

-- ========== Entity table ==========
DEFINE TABLE entity SCHEMAFULL;

-- Core fields
DEFINE FIELD name ON entity TYPE string;
DEFINE FIELD entity_type ON entity TYPE string DEFAULT "concept";
DEFINE FIELD project ON entity TYPE option<string>;
DEFINE FIELD confidence ON entity TYPE float DEFAULT 1.0;

-- Aliases for entity resolution
DEFINE FIELD aliases ON entity TYPE array DEFAULT [];
DEFINE FIELD aliases.* ON entity TYPE string;

-- Provenance
DEFINE FIELD source_episodes ON entity TYPE array DEFAULT [];
DEFINE FIELD source_episodes.* ON entity TYPE object;
DEFINE FIELD source_episodes.*.episode_type ON entity TYPE string;
DEFINE FIELD source_episodes.*.episode_id ON entity TYPE string;

-- Timestamps
DEFINE FIELD created_at ON entity TYPE datetime DEFAULT time::now();
DEFINE FIELD updated_at ON entity TYPE datetime DEFAULT time::now();

-- Indexes
DEFINE INDEX entity_name ON entity FIELDS name;
DEFINE INDEX entity_project ON entity FIELDS project;
DEFINE INDEX entity_type ON entity FIELDS entity_type;

-- Unique constraint: name + project should be unique
DEFINE INDEX entity_unique ON entity FIELDS name, project UNIQUE;

-- ========== Fact-Entity relationship ==========
DEFINE TABLE fact_entity SCHEMAFULL;
DEFINE FIELD fact ON fact_entity TYPE record<fact>;
DEFINE FIELD entity ON fact_entity TYPE record<entity>;
DEFINE FIELD role ON fact_entity TYPE string DEFAULT "subject";
DEFINE INDEX fact_entity_fact ON fact_entity FIELDS fact;
DEFINE INDEX fact_entity_entity ON fact_entity FIELDS entity;

-- ========== Processing state table ==========
-- Tracks which episodes have been processed for extraction
DEFINE TABLE extraction_state SCHEMAFULL;
DEFINE FIELD episode_type ON extraction_state TYPE string;
DEFINE FIELD last_processed_id ON extraction_state TYPE option<string>;
DEFINE FIELD last_processed_at ON extraction_state TYPE option<datetime>;
DEFINE FIELD processing_version ON extraction_state TYPE int DEFAULT 1;
DEFINE INDEX extraction_state_type ON extraction_state FIELDS episode_type UNIQUE;
