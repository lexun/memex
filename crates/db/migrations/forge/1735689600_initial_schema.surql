-- Forge Initial Schema
-- Task management tables and indexes

-- Task table
DEFINE TABLE task SCHEMAFULL;
DEFINE FIELD title ON task TYPE string;
DEFINE FIELD description ON task TYPE option<string>;
DEFINE FIELD status ON task TYPE string DEFAULT 'pending';
DEFINE FIELD project ON task TYPE option<string>;
DEFINE FIELD priority ON task TYPE int DEFAULT 0;
DEFINE FIELD created_at ON task TYPE datetime DEFAULT time::now();
DEFINE FIELD updated_at ON task TYPE datetime DEFAULT time::now();
DEFINE FIELD completed_at ON task TYPE option<datetime>;

-- Indexes for common queries
DEFINE INDEX task_status ON task FIELDS status;
DEFINE INDEX task_project ON task FIELDS project;
DEFINE INDEX task_priority ON task FIELDS priority;

-- Task notes/updates
DEFINE TABLE task_note SCHEMAFULL;
DEFINE FIELD task_id ON task_note TYPE record<task>;
DEFINE FIELD content ON task_note TYPE string;
DEFINE FIELD created_at ON task_note TYPE datetime DEFAULT time::now();
DEFINE FIELD updated_at ON task_note TYPE datetime DEFAULT time::now();

DEFINE INDEX task_note_task ON task_note FIELDS task_id;

-- Task dependencies (relationships between tasks)
DEFINE TABLE task_dependency SCHEMAFULL;
DEFINE FIELD from_task ON task_dependency TYPE record<task>;
DEFINE FIELD to_task ON task_dependency TYPE record<task>;
DEFINE FIELD relation ON task_dependency TYPE string DEFAULT 'blocks';
DEFINE FIELD created_at ON task_dependency TYPE datetime DEFAULT time::now();

DEFINE INDEX task_dep_from ON task_dependency FIELDS from_task;
DEFINE INDEX task_dep_to ON task_dependency FIELDS to_task;
