DO $$
BEGIN
    RAISE EXCEPTION
        '2026-04-23-120000_drop_legacy_tables is irreversible; restore dropped legacy tables from backup/export instead';
END
$$;
