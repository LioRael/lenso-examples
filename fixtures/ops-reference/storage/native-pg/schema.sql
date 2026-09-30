CREATE TABLE schema_version (
    singleton boolean PRIMARY KEY CHECK (singleton),
    version bigint NOT NULL
);
INSERT INTO schema_version VALUES (true, 1);
CREATE TABLE state (
    singleton boolean PRIMARY KEY CHECK (singleton),
    label text NOT NULL,
    value bigint NOT NULL CHECK (value BETWEEN -9007199254740991 AND 9007199254740991),
    revision bigint NOT NULL CHECK (revision BETWEEN 0 AND 9007199254740991)
);
CREATE TABLE receipts (
    idempotency_key text PRIMARY KEY,
    intent_digest text NOT NULL,
    label text NOT NULL,
    value bigint NOT NULL CHECK (value BETWEEN -9007199254740991 AND 9007199254740991),
    revision bigint NOT NULL CHECK (revision BETWEEN 1 AND 9007199254740991),
    receipt_id text NOT NULL UNIQUE
);
