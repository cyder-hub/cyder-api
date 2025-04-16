ALTER TABLE api_keys DROP COLUMN limit_strategy_id;

DROP TABLE IF EXISTS limit_strategy_item;

DROP TABLE IF EXISTS limit_strategy;
