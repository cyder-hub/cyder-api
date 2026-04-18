DROP INDEX IF EXISTS idx_api_key_model_override_target_route_id;
DROP INDEX IF EXISTS idx_api_key_model_override_api_key_id;
DROP INDEX IF EXISTS idx_api_key_model_override_name_uq_active;
DROP TABLE IF EXISTS api_key_model_override;

DROP INDEX IF EXISTS idx_model_route_candidate_route_priority;
DROP INDEX IF EXISTS idx_model_route_candidate_route_id;
DROP INDEX IF EXISTS idx_model_route_candidate_route_model_uq_active;
DROP TABLE IF EXISTS model_route_candidate;

DROP INDEX IF EXISTS idx_model_route_enabled;
DROP INDEX IF EXISTS idx_model_route_deleted_at;
DROP INDEX IF EXISTS idx_model_route_name_uq_active;
DROP TABLE IF EXISTS model_route;
