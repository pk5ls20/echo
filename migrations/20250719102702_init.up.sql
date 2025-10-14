-- Add up migration script here
PRAGMA foreign_keys = ON;

CREATE TABLE permissions
(
    id          INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    description TEXT    NOT NULL,
    color       INTEGER NOT NULL UNIQUE
);

CREATE TABLE resources
(
    id          INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    uploader_id INTEGER NOT NULL REFERENCES users (id), -- TODO: maybe change?
    res_name    TEXT    NOT NULL,        -- user provided original name
    res_uuid    BLOB    NOT NULL UNIQUE, -- res uuid
    res_ext     TEXT    NOT NULL         -- file extension
);
CREATE TABLE resource_references
(
    id          INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    res_id      INTEGER NOT NULL UNIQUE REFERENCES resources (id), -- TODO: maybe change?
    target_id   INTEGER NOT NULL,
    target_type INTEGER NOT NULL -- id of the target table
);
CREATE INDEX idx_res_ref_target ON resource_references (target_id, target_type);
CREATE INDEX idx_res_ref_res_target ON resource_references (res_id, target_id, target_type);

CREATE TABLE users
(
    id            INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    username      TEXT    NOT NULL UNIQUE,
    password_hash TEXT    NOT NULL,
    role          INTEGER NOT NULL,
    created_at    INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    avatar_res_id INTEGER REFERENCES resources (id) ON DELETE SET NULL -- TODO: not ok, use <resource_references>
);
CREATE TABLE user_permissions
(
    id            INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    user_id       INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    permission_id INTEGER NOT NULL REFERENCES permissions (id) ON DELETE CASCADE,
    assigner_id   INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    assigned_at   INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    exp_time      INTEGER,
    revoked_at    INTEGER NULL,
    active        INTEGER
        GENERATED ALWAYS AS (
            CASE
                WHEN revoked_at IS NULL
                    AND (exp_time IS NULL OR exp_time > CAST(strftime('%s', 'now') AS INTEGER))
                    THEN 1
                ELSE 0
                END
            ) VIRTUAL     NOT NULL
);
CREATE INDEX idx_user_permissions_user_active ON user_permissions (user_id, active);
CREATE INDEX idx_user_permissions_user_perm ON user_permissions (user_id, permission_id);

CREATE TABLE echos
(
    id               INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    user_id          INTEGER NOT NULL REFERENCES users (id), -- ok, always use soft delete for users
    content          TEXT    NOT NULL,
    fav_count        INTEGER NOT NULL DEFAULT 0,
    is_private       INTEGER NOT NULL DEFAULT 0,
    created_at       INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    last_modified_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);
CREATE INDEX idx_echos_user_id_id ON echos (user_id, id);
CREATE TABLE echo_permissions
(
    echo_id       INTEGER NOT NULL REFERENCES echos (id) ON DELETE CASCADE,
    permission_id INTEGER NOT NULL REFERENCES permissions (id) ON DELETE CASCADE,
    PRIMARY KEY (echo_id, permission_id)
);

CREATE TABLE auth_tokens
(
    id           INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token        TEXT    NOT NULL UNIQUE,
    created_at   INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    exp_time     INTEGER NOT NULL,
    last_used_at INTEGER NULL
);
CREATE INDEX idx_auth_tokens_user_id ON auth_tokens (user_id);

CREATE TABLE invite_codes
(
    id         INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    code       TEXT    NOT NULL UNIQUE,
    issued_by  INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    exp_time   INTEGER NOT NULL,
    is_used    INTEGER NOT NULL DEFAULT 0,
    used_by    INTEGER NULL REFERENCES users (id) ON DELETE CASCADE,
    used_at    INTEGER NULL
);

CREATE TABLE system_settings
(
    id           INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    key          TEXT    NOT NULL UNIQUE,
    val          TEXT    NOT NULL,
    created_at   INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at   INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE TABLE mfa_infos
(
    user_id     INTEGER NOT NULL PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    mfa_enabled INTEGER NOT NULL DEFAULT 0,
    updated_at  INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE TABLE totp_credentials
(
    id                   INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    user_id              INTEGER NOT NULL UNIQUE REFERENCES users (id) ON DELETE CASCADE,
    totp_credential_data BLOB    NOT NULL,
    created_at           INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at           INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    last_used_at         INTEGER NULL
);

CREATE TABLE webauthn_credentials
(
    id                INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    user_id           INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    user_unique_uuid  BLOB    NOT NULL UNIQUE,
    user_name         TEXT    NOT NULL,
    user_display_name TEXT    NULL,
    credential_data   BLOB    NOT NULL,
    created_at        INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    updated_at        INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    last_used_at      INTEGER NULL
);
CREATE INDEX idx_webauthn_user_created_at ON webauthn_credentials (user_id, created_at DESC);

CREATE TABLE mfa_op_logs
(
    id            INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    user_id       INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    op_type       INTEGER NOT NULL,
    auth_method   INTEGER NOT NULL,
    is_success    INTEGER NOT NULL,
    ip_address    TEXT    NULL,
    user_agent    TEXT    NULL,
    credential_id INTEGER NULL,
    error_message TEXT    NULL,
    time          INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);
CREATE INDEX idx_mfa_op_logs_user_id_id ON mfa_op_logs (user_id, id);
