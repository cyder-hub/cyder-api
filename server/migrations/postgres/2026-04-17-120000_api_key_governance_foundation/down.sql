DROP INDEX IF EXISTS idx_api_key_rollup_monthly_bucket;
DROP INDEX IF EXISTS idx_api_key_rollup_daily_bucket;
DROP INDEX IF EXISTS idx_api_key_acl_rule_api_key_id;
DROP INDEX IF EXISTS idx_api_key_acl_rule_logical_key;
DROP INDEX IF EXISTS idx_api_key_expires_at;
DROP INDEX IF EXISTS idx_api_key_deleted_at;
DROP INDEX IF EXISTS idx_api_key_name;
DROP INDEX IF EXISTS idx_api_key_hash_uq_active;
DROP INDEX IF EXISTS idx_api_key_key_uq_active;

DROP TABLE IF EXISTS api_key_rollup_monthly;
DROP TABLE IF EXISTS api_key_rollup_daily;
DROP TABLE IF EXISTS api_key_acl_rule;
DROP TABLE IF EXISTS api_key;
