-- Migration 002: Memo schema for Atlas
-- Memos are standalone pieces of information recorded on request

DEFINE TABLE memo SCHEMAFULL;

-- When the memo was created
DEFINE FIELD created_at ON memo TYPE datetime DEFAULT time::now();

-- The memo content
DEFINE FIELD content ON memo TYPE string;

-- Source attribution (nested object)
DEFINE FIELD source ON memo TYPE object;
DEFINE FIELD source.actor ON memo TYPE string;
DEFINE FIELD source.authority ON memo TYPE string;
DEFINE FIELD source.context ON memo TYPE option<object>;
DEFINE FIELD source.context.via ON memo TYPE option<string>;

-- Indexes for efficient queries
DEFINE INDEX memo_created ON memo FIELDS created_at;
