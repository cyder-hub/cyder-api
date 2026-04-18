ALTER TABLE request_log
ADD COLUMN requested_model_name TEXT NULL;

ALTER TABLE request_log
ADD COLUMN resolved_name_scope TEXT NULL;

ALTER TABLE request_log
ADD COLUMN resolved_route_id BIGINT NULL;

ALTER TABLE request_log
ADD COLUMN resolved_route_name TEXT NULL;
