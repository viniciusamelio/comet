# Comet Auth Setup

This guide covers the current `comet-auth` setup for Cloudflare Workers.
The auth runtime stores users, linked provider accounts, and sessions in D1.
KV is used for OAuth state and can be used as a fast session cache; D1 remains
the source of truth.

## Worker Bindings

Your Worker needs one D1 binding and one KV namespace:

```jsonc
{
  "d1_databases": [
    {
      "binding": "DB",
      "database_name": "my-app",
      "database_id": "...",
      "migrations_dir": "migrations"
    }
  ],
  "kv_namespaces": [
    {
      "binding": "AUTH_KV",
      "id": "..."
    }
  ]
}
```

Add the auth migration with:

```sh
comet auth init --db-binding DB --kv-binding AUTH_KV
```

## Rocket Mount

```rust
pub struct DB;
impl comet::cloudflare::BindingName for DB {
    const NAME: &'static str = "DB";
}

pub struct AuthKv;
impl comet::cloudflare::BindingName for AuthKv {
    const NAME: &'static str = "AUTH_KV";
}

let auth_config = comet_auth::AuthConfig::from_env()
    .base_url("https://example.com")
    .provider(
        comet_auth::providers::Google::from_env()
            .web_client_id_env("GOOGLE_WEB_CLIENT_ID")
            .web_client_secret_env("GOOGLE_WEB_CLIENT_SECRET")
            .native_client_id_env("GOOGLE_IOS_CLIENT_ID")
            .native_client_id_env("GOOGLE_ANDROID_CLIENT_ID"),
    )
    .provider(
        comet_auth::providers::Apple::from_env()
            .service_id_env("APPLE_SERVICE_ID")
            .team_id_env("APPLE_TEAM_ID")
            .key_id_env("APPLE_KEY_ID")
            .private_key_pkcs8_pem_env("APPLE_PRIVATE_KEY_PKCS8_PEM")
            .native_audience_env("APPLE_IOS_BUNDLE_ID"),
    )
    .provider(
        comet_auth::providers::GitHub::from_env()
            .client_id_env("GITHUB_CLIENT_ID")
            .client_secret_env("GITHUB_CLIENT_SECRET"),
    );

rocket::build()
    .attach(comet_auth::Auth::<DB, AuthKv>::fairing(auth_config))
    .mount("/auth", comet_auth::routes::<DB, AuthKv>());
```

## Protected Routes

Put `#[comet_auth::requires_auth]` above the Rocket route attribute:

```rust
#[comet_auth::requires_auth]
#[rocket::get("/private/me")]
async fn private_me(session: comet_auth::AuthSession) -> &'static str {
    "authenticated"
}
```

For anonymous-aware routes:

```rust
#[comet_auth::requires_auth(optional)]
#[rocket::get("/maybe")]
async fn maybe(session: comet_auth::OptionalAuthSession) -> &'static str {
    if session.0.is_some() { "signed in" } else { "anonymous" }
}
```

`scope = "..."` is accepted today and behaves as required authentication.
RBAC enforcement is planned for phase 7.

## Provider Secrets

Set secrets with `wrangler secret put <NAME>` for deployed Workers. For local
development, use Wrangler's local secret flow or `.dev.vars` according to your
project policy.

Common secrets:

- `COMET_AUTH_BASE_URL`: public origin used to build OAuth callback URLs.
- `COMET_AUTH_TOKEN_PEPPER`: extra secret material mixed into session token
  hashes.

Google:

- `GOOGLE_WEB_CLIENT_ID`
- `GOOGLE_WEB_CLIENT_SECRET`
- `GOOGLE_IOS_CLIENT_ID`, optional native audience
- `GOOGLE_ANDROID_CLIENT_ID`, optional native audience

Apple:

- `APPLE_SERVICE_ID`, used for web OAuth audience/client id
- `APPLE_TEAM_ID`
- `APPLE_KEY_ID`
- `APPLE_PRIVATE_KEY_PKCS8_PEM`
- `APPLE_IOS_BUNDLE_ID`, optional native audience

GitHub:

- `GITHUB_CLIENT_ID`
- `GITHUB_CLIENT_SECRET`

## Redirect URIs

Configure these callback URLs in each provider dashboard:

- Google: `<COMET_AUTH_BASE_URL>/auth/google/callback`
- Apple: `<COMET_AUTH_BASE_URL>/auth/apple/callback`
- GitHub: `<COMET_AUTH_BASE_URL>/auth/github/callback`

## Native Login

Native clients should use provider-native SDKs to obtain an identity token,
then send it to Comet:

```sh
curl -X POST https://example.com/auth/native/google \
  -H 'content-type: application/json' \
  -d '{"id_token":"...","nonce":"..."}'
```

Apple uses the same request shape at `/auth/native/apple`.

Do not use an embedded WebView for Google login. For browser-based login from a
mobile app, use the system browser flow such as `ASWebAuthenticationSession`,
`SFSafariViewController`, or Chrome Custom Tabs.
