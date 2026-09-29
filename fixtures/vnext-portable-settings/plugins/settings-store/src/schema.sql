CREATE TABLE IF NOT EXISTS portable_settings (
    principal TEXT PRIMARY KEY,
    revision BIGINT NOT NULL CHECK (revision BETWEEN 0 AND 2147483647),
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS portable_settings_receipts (
    principal TEXT NOT NULL REFERENCES portable_settings(principal),
    idempotency_key TEXT NOT NULL,
    request JSONB NOT NULL,
    response JSONB NOT NULL,
    PRIMARY KEY (principal, idempotency_key)
);
