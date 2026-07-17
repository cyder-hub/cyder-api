DO $$
BEGIN
    RAISE EXCEPTION 'irreversible migration: retired routing, attempt, replay, and diagnostic data cannot be restored';
END
$$;
