-- The factory's change feed (factory_core::feed). Each graph's latest change
-- is an olai-store object labeled `feed`, whose version is the graph's
-- sequence. Writing one notifies channel `factory` from inside the writing
-- transaction, so a notification is sent exactly when the entry commits.
--
-- The payload is small (well under NOTIFY's 8000 bytes): the graph's root ref,
-- the sequence, and the writer's origin, so a server can skip its own writes.
-- Listeners read what changed from the store.

CREATE OR REPLACE FUNCTION factory_feed_notify() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_notify('factory', json_build_object(
        'root', NEW.properties ->> 'root',
        'seq', NEW.version,
        'origin', NEW.properties ->> 'origin'
    )::text);
    RETURN NULL;
END
$$;

CREATE TRIGGER factory_feed_notify
    AFTER INSERT OR UPDATE ON objects
    FOR EACH ROW
    WHEN (NEW.label = 'feed')
    EXECUTE FUNCTION factory_feed_notify();
