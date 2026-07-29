-- This migration intentionally destroys retired routing, attempt, replay, and
-- diagnostic data. Rolling it back cannot restore the removed semantics.
SELECT * FROM cyder_irreversible_remove_routing_attempt_replay_diagnostics;
