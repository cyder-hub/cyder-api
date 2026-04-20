ALTER TABLE request_log
ADD COLUMN requested_model_name TEXT;

ALTER TABLE request_log
ADD COLUMN resolved_name_scope TEXT;

ALTER TABLE request_log
ADD COLUMN resolved_route_id BIGINT;

ALTER TABLE request_log
ADD COLUMN resolved_route_name TEXT;
