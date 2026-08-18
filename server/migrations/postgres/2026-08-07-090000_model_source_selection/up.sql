-- R3.11 makes Model Source availability explicit and removes the retired
-- Model-owned upstream capability flags. This migration has no down migration.

ALTER TABLE model
    ADD COLUMN source_selection_mode TEXT NOT NULL DEFAULT 'INHERIT_ALL',
    ADD CONSTRAINT chk_model_source_selection_mode CHECK (
        source_selection_mode IN ('INHERIT_ALL', 'EXPLICIT')
    );

ALTER TABLE model
    ALTER COLUMN source_selection_mode DROP DEFAULT,
    DROP COLUMN supports_streaming,
    DROP COLUMN supports_tools,
    DROP COLUMN supports_reasoning,
    DROP COLUMN supports_image_input,
    DROP COLUMN supports_embeddings,
    DROP COLUMN supports_rerank;

CREATE TABLE model_source_binding (
    model_id BIGINT NOT NULL,
    source_id BIGINT NOT NULL,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (model_id, source_id),
    CONSTRAINT fk_model_source_binding_model_id
        FOREIGN KEY (model_id) REFERENCES model (id)
            ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT fk_model_source_binding_source_id
        FOREIGN KEY (source_id) REFERENCES upstream_source (id)
            ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT chk_model_source_binding_timestamps CHECK (updated_at >= created_at)
);

CREATE INDEX idx_model_source_binding_source_id
    ON model_source_binding (source_id);
CREATE UNIQUE INDEX idx_model_source_binding_model_default_unique
    ON model_source_binding (model_id)
    WHERE is_default = TRUE;

ALTER TABLE request_log
    ADD COLUMN source_selection_reason TEXT NULL,
    ADD CONSTRAINT chk_request_log_source_selection_reason CHECK (
        source_selection_reason IS NULL
        OR source_selection_reason IN (
            'protocol_match',
            'provider_default_transform',
            'model_default_transform'
        )
    );
