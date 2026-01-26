-- Fact-Record linkage for Entity→Record consolidation
-- Links facts to Records instead of Entities for unified identity
-- Part of Entity→Record migration (Phase 3)

-- ========== Fact-Record relationship ==========
-- This table links Facts to Records, replacing fact_entity for new extractions
-- Both tables coexist during migration: fact_entity for legacy, fact_record for new

DEFINE TABLE fact_record SCHEMAFULL;

-- Foreign key references
DEFINE FIELD fact ON fact_record TYPE record<fact>;
DEFINE FIELD record ON fact_record TYPE record<record>;

-- Role describes how the record relates to the fact
-- Common roles: "subject", "object", "mentions", "about"
DEFINE FIELD role ON fact_record TYPE string DEFAULT "mentions";

-- Confidence score for the link (inherited from extraction)
DEFINE FIELD confidence ON fact_record TYPE float DEFAULT 1.0;

-- Timestamp for when this link was created
DEFINE FIELD created_at ON fact_record TYPE datetime DEFAULT time::now();

-- Indexes for efficient querying
DEFINE INDEX fact_record_fact ON fact_record FIELDS fact;
DEFINE INDEX fact_record_record ON fact_record FIELDS record;

-- Composite index for checking if link exists
DEFINE INDEX fact_record_unique ON fact_record FIELDS fact, record, role UNIQUE;
