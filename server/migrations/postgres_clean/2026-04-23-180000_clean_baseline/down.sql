DO $$
BEGIN
    RAISE EXCEPTION
        '2026-04-23-180000_clean_baseline is not reversible; recreate the database instead';
END
$$;
