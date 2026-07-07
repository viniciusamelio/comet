CREATE TABLE IF NOT EXISTS comet_auth_roles (
    name TEXT PRIMARY KEY,
    description TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS comet_auth_permissions (
    name TEXT PRIMARY KEY,
    description TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS comet_auth_user_roles (
    user_id TEXT NOT NULL REFERENCES comet_auth_users(id) ON DELETE CASCADE,
    role_name TEXT NOT NULL REFERENCES comet_auth_roles(name) ON DELETE CASCADE,
    resource TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, role_name, resource)
);

CREATE INDEX IF NOT EXISTS comet_auth_user_roles_user_id_idx
    ON comet_auth_user_roles(user_id);

CREATE INDEX IF NOT EXISTS comet_auth_user_roles_role_name_idx
    ON comet_auth_user_roles(role_name);

CREATE TABLE IF NOT EXISTS comet_auth_role_permissions (
    role_name TEXT NOT NULL REFERENCES comet_auth_roles(name) ON DELETE CASCADE,
    permission_name TEXT NOT NULL REFERENCES comet_auth_permissions(name) ON DELETE CASCADE,
    resource TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    PRIMARY KEY (role_name, permission_name, resource)
);

CREATE INDEX IF NOT EXISTS comet_auth_role_permissions_role_name_idx
    ON comet_auth_role_permissions(role_name);

CREATE TABLE IF NOT EXISTS comet_auth_user_permissions (
    user_id TEXT NOT NULL REFERENCES comet_auth_users(id) ON DELETE CASCADE,
    permission_name TEXT NOT NULL REFERENCES comet_auth_permissions(name) ON DELETE CASCADE,
    resource TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, permission_name, resource)
);

CREATE INDEX IF NOT EXISTS comet_auth_user_permissions_user_id_idx
    ON comet_auth_user_permissions(user_id);

CREATE INDEX IF NOT EXISTS comet_auth_user_permissions_permission_name_idx
    ON comet_auth_user_permissions(permission_name);
