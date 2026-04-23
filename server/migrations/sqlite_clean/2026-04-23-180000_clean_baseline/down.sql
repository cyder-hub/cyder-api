CREATE TEMP TABLE __clean_baseline_irreversible_guard__ (
    id INTEGER PRIMARY KEY
);

CREATE TEMP TRIGGER __clean_baseline_irreversible_guard_abort__
BEFORE INSERT ON __clean_baseline_irreversible_guard__
BEGIN
    SELECT RAISE(
        FAIL,
        '2026-04-23-180000_clean_baseline is not reversible; recreate the database instead'
    );
END;

INSERT INTO __clean_baseline_irreversible_guard__ (id) VALUES (1);
