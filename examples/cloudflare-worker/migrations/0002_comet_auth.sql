CREATE TABLE IF NOT EXISTS comet_auth_users (
    id TEXT PRIMARY KEY,
    primary_email TEXT,
    email_verified INTEGER NOT NULL DEFAULT 0,
    name TEXT,
    avatar_url TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS comet_auth_accounts (
    provider TEXT NOT NULL,
    provider_account_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    email TEXT,
    email_verified INTEGER NOT NULL DEFAULT 0,
    name TEXT,
    avatar_url TEXT,
    raw_profile_json TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (provider, provider_account_id),
    FOREIGN KEY (user_id) REFERENCES comet_auth_users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_comet_auth_accounts_user_id
    ON comet_auth_accounts(user_id);

CREATE TABLE IF NOT EXISTS comet_auth_sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    revoked_at INTEGER,
    user_agent_hash TEXT,
    ip_hash TEXT,
    FOREIGN KEY (user_id) REFERENCES comet_auth_users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_comet_auth_sessions_user_id
    ON comet_auth_sessions(user_id);

CREATE INDEX IF NOT EXISTS idx_comet_auth_sessions_token_hash
    ON comet_auth_sessions(token_hash);

CREATE INDEX IF NOT EXISTS idx_comet_auth_sessions_expires_at
    ON comet_auth_sessions(expires_at);
