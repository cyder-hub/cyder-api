CREATE TEMP TABLE __drop_legacy_tables_irreversible_guard__ (
    id INTEGER PRIMARY KEY
);

CREATE TEMP TRIGGER __drop_legacy_tables_irreversible_guard_abort__
BEFORE INSERT ON __drop_legacy_tables_irreversible_guard__
BEGIN
    SELECT RAISE(
        FAIL,
        '2026-04-23-120000_drop_legacy_tables is irreversible; restore dropped legacy tables from backup/export instead'
    );
END;

INSERT INTO __drop_legacy_tables_irreversible_guard__ (id) VALUES (1);
