-- Forge Full-Text Search
-- Add search analyzer and indexes for task content

-- Define a text analyzer for English content
DEFINE ANALYZER task_analyzer TOKENIZERS class FILTERS lowercase, snowball(english);

-- Full-text search index on task title and description
DEFINE INDEX task_title_search ON task FIELDS title SEARCH ANALYZER task_analyzer BM25;
DEFINE INDEX task_desc_search ON task FIELDS description SEARCH ANALYZER task_analyzer BM25;

-- Full-text search index on task note content
DEFINE INDEX task_note_content_search ON task_note FIELDS content SEARCH ANALYZER task_analyzer BM25;
