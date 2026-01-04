-- Atlas Full-Text Search
-- Add search analyzer and indexes for memo content

-- Define a text analyzer for English content
DEFINE ANALYZER memo_analyzer TOKENIZERS class FILTERS lowercase, snowball(english);

-- Full-text search index on memo content
DEFINE INDEX memo_content_search ON memo FIELDS content SEARCH ANALYZER memo_analyzer BM25;
