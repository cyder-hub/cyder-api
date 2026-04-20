ALTER TABLE request_log
DROP COLUMN requested_model_name;

ALTER TABLE request_log
DROP COLUMN resolved_name_scope;

ALTER TABLE request_log
DROP COLUMN resolved_route_id;

ALTER TABLE request_log
DROP COLUMN resolved_route_name;
