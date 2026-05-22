CREATE TABLE one_time_kyber_prekeys (
    id          INTEGER NOT NULL,
    device_pk   BIGINT  NOT NULL REFERENCES devices(id),
    public_key  BYTEA   NOT NULL,
    signature   BYTEA   NOT NULL,
    uploaded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (device_pk, id)
);
