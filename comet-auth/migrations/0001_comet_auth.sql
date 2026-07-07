CREATE TABLE IF NOT EXISTS comet_auth_users (
    id TEXT PRIMARY KEY,
    primary_email TEXT,
    email_verified INTEGER NOT NULL DEFAULT 0,
    name TEXT,
    avatar_url TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS comet_auth_users_primary_email_unique
    ON comet_auth_users(primary_email)
    WHERE primary_email IS NOT NULL;

CREATE TABLE IF NOT EXISTS comet_auth_accounts (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES comet_auth_users(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    provider_account_id TEXT NOT NULL,
    email TEXT,
    email_verified INTEGER NOT NULL DEFAULT 0,
    name TEXT,
    avatar_url TEXT,
    raw_profile_json TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(provider, provider_account_id)
);

CREATE INDEX IF NOT EXISTS comet_auth_accounts_user_id_idx
    ON comet_auth_accounts(user_id);

CREATE TABLE IF NOT EXISTS comet_auth_sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES comet_auth_users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    revoked_at INTEGER,
    user_agent_hash TEXT,
    ip_hash TEXT
);

CREATE INDEX IF NOT EXISTS comet_auth_sessions_user_id_idx
    ON comet_auth_sessions(user_id);

CREATE INDEX IF NOT EXISTS comet_auth_sessions_expires_at_idx
    ON comet_auth_sessions(expires_at);

CREATE TABLE IF NOT EXISTS comet_auth_oauth_states (
    state_hash TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    code_verifier TEXT NOT NULL,
    nonce TEXT,
    redirect_after TEXT,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS comet_auth_oauth_states_expires_at_idx
    ON comet_auth_oauth_states(expires_at);
