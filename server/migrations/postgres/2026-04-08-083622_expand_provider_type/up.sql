-- enum add action should not be revert in down.sql
ALTER TYPE llm_api_type_enum ADD VALUE 'GEMINI_OPENAI';
ALTER TYPE provider_type_enum ADD VALUE 'GEMINI_OPENAI';
