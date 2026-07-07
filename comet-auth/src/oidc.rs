#![cfg_attr(not(feature = "cloudflare"), allow(dead_code))]

use base64ct::{Base64UrlUnpadded, Encoding};
use crypto_bigint::BoxedUint;
use rsa::RsaPublicKey;
use rsa::pkcs1v15::{Signature as RsaSignature, VerifyingKey};
use rsa::signature::Verifier;
use serde::Deserialize;
use sha2::Sha256;

use crate::AuthError;
use crate::session;

const CLOCK_SKEW_SECONDS: i64 = 60;

#[derive(Debug, Clone, Deserialize)]
pub struct OidcClaims {
    pub iss: String,
    pub sub: String,
    pub aud: Audience,
    pub exp: i64,
    pub iat: Option<i64>,
    pub nonce: Option<String>,
    pub email: Option<String>,
    pub email_verified: Option<EmailVerified>,
    pub name: Option<String>,
    pub picture: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Audience {
    Single(String),
    Multiple(Vec<String>),
}

impl Audience {
    fn contains_any(&self, allowed: &[String]) -> bool {
        match self {
            Audience::Single(value) => allowed.iter().any(|allowed| allowed == value),
            Audience::Multiple(values) => values
                .iter()
                .any(|value| allowed.iter().any(|allowed| allowed == value)),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum EmailVerified {
    Bool(bool),
    String(String),
}

impl EmailVerified {
    pub fn as_bool(&self) -> bool {
        match self {
            EmailVerified::Bool(value) => *value,
            EmailVerified::String(value) => value == "true" || value == "1",
        }
    }
}

#[derive(Debug, Deserialize)]
struct JwtHeader {
    alg: String,
    kid: String,
}

#[derive(Debug, Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Debug, Deserialize)]
struct Jwk {
    kid: String,
    kty: String,
    alg: Option<String>,
    n: String,
    e: String,
}

pub struct OidcValidation<'a> {
    pub issuer: &'a str,
    pub audiences: &'a [String],
    pub expected_nonce: Option<&'a str>,
}

#[cfg(feature = "cloudflare")]
pub async fn validate_rs256_id_token(
    jwks_url: &str,
    id_token: &str,
    validation: OidcValidation<'_>,
) -> Result<OidcClaims, AuthError> {
    let (header, claims, signing_input, signature) = decode_jwt_parts(id_token)?;
    if header.alg != "RS256" {
        return Err(AuthError::InvalidIdentityToken(format!(
            "unsupported JWT alg `{}`",
            header.alg
        )));
    }

    let jwks = fetch_jwks(jwks_url).await?;
    let jwk = jwks
        .keys
        .iter()
        .find(|key| key.kid == header.kid)
        .ok_or_else(|| AuthError::InvalidIdentityToken("matching JWK not found".into()))?;
    verify_jwk(jwk, signing_input.as_bytes(), &signature)?;
    validate_claims(&claims, validation)?;
    Ok(claims)
}

#[cfg(feature = "cloudflare")]
async fn fetch_jwks(url: &str) -> Result<Jwks, AuthError> {
    let mut response = worker::Fetch::Url(
        url::Url::parse(url).map_err(|error| AuthError::ProviderRequest(error.to_string()))?,
    )
    .send()
    .await?;
    if response.status_code() >= 400 {
        return Err(AuthError::ProviderRequest(response.text().await?));
    }
    response.json::<Jwks>().await.map_err(AuthError::from)
}

fn decode_jwt_parts(jwt: &str) -> Result<(JwtHeader, OidcClaims, String, Vec<u8>), AuthError> {
    let mut parts = jwt.split('.');
    let header = parts
        .next()
        .ok_or_else(|| AuthError::InvalidIdentityToken("missing JWT header".into()))?;
    let payload = parts
        .next()
        .ok_or_else(|| AuthError::InvalidIdentityToken("missing JWT payload".into()))?;
    let signature = parts
        .next()
        .ok_or_else(|| AuthError::InvalidIdentityToken("missing JWT signature".into()))?;
    if parts.next().is_some() {
        return Err(AuthError::InvalidIdentityToken("too many JWT parts".into()));
    }

    let header_value = decode_base64_json::<JwtHeader>(header)?;
    let claims = decode_base64_json::<OidcClaims>(payload)?;
    let signature = Base64UrlUnpadded::decode_vec(signature)
        .map_err(|_| AuthError::InvalidIdentityToken("malformed JWT signature".into()))?;
    Ok((
        header_value,
        claims,
        format!("{header}.{payload}"),
        signature,
    ))
}

fn decode_base64_json<T: for<'de> Deserialize<'de>>(value: &str) -> Result<T, AuthError> {
    let bytes = Base64UrlUnpadded::decode_vec(value)
        .map_err(|_| AuthError::InvalidIdentityToken("malformed JWT base64".into()))?;
    serde_json::from_slice(&bytes).map_err(AuthError::from)
}

fn verify_jwk(jwk: &Jwk, signing_input: &[u8], signature: &[u8]) -> Result<(), AuthError> {
    if jwk.kty != "RSA" || jwk.alg.as_deref().is_some_and(|alg| alg != "RS256") {
        return Err(AuthError::InvalidIdentityToken(
            "JWK is not an RS256 RSA key".into(),
        ));
    }

    let n_bytes = Base64UrlUnpadded::decode_vec(&jwk.n)
        .map_err(|_| AuthError::InvalidIdentityToken("malformed JWK modulus".into()))?;
    let e_bytes = Base64UrlUnpadded::decode_vec(&jwk.e)
        .map_err(|_| AuthError::InvalidIdentityToken("malformed JWK exponent".into()))?;
    let n = BoxedUint::from_be_slice(&n_bytes, (n_bytes.len() * 8) as u32)
        .map_err(|error| AuthError::InvalidIdentityToken(error.to_string()))?;
    let e = BoxedUint::from_be_slice(&e_bytes, (e_bytes.len() * 8) as u32)
        .map_err(|error| AuthError::InvalidIdentityToken(error.to_string()))?;
    let public_key = RsaPublicKey::new(n, e)
        .map_err(|error| AuthError::InvalidIdentityToken(error.to_string()))?;
    let signature = RsaSignature::try_from(signature)
        .map_err(|_| AuthError::InvalidIdentityToken("malformed RS256 signature".into()))?;
    VerifyingKey::<Sha256>::new(public_key)
        .verify(signing_input, &signature)
        .map_err(|_| AuthError::InvalidIdentityToken("JWT signature verification failed".into()))
}

pub fn validate_claims(
    claims: &OidcClaims,
    validation: OidcValidation<'_>,
) -> Result<(), AuthError> {
    if claims.iss != validation.issuer {
        return Err(AuthError::InvalidIdentityToken("issuer mismatch".into()));
    }
    if !claims.aud.contains_any(validation.audiences) {
        return Err(AuthError::InvalidIdentityToken("audience mismatch".into()));
    }
    if let Some(expected_nonce) = validation.expected_nonce {
        if claims.nonce.as_deref() != Some(expected_nonce) {
            return Err(AuthError::InvalidIdentityToken("nonce mismatch".into()));
        }
    }
    let now = session::now_unix();
    if claims.exp + CLOCK_SKEW_SECONDS <= now {
        return Err(AuthError::InvalidIdentityToken("token expired".into()));
    }
    if claims.iat.is_some_and(|iat| iat > now + CLOCK_SKEW_SECONDS) {
        return Err(AuthError::InvalidIdentityToken(
            "token issued in the future".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Audience, OidcClaims, OidcValidation, validate_claims};

    fn claims() -> OidcClaims {
        OidcClaims {
            iss: "https://accounts.google.com".into(),
            sub: "sub".into(),
            aud: Audience::Single("client".into()),
            exp: crate::session::now_unix() + 600,
            iat: Some(crate::session::now_unix()),
            nonce: Some("nonce".into()),
            email: None,
            email_verified: None,
            name: None,
            picture: None,
        }
    }

    #[test]
    fn validates_claims() {
        validate_claims(
            &claims(),
            OidcValidation {
                issuer: "https://accounts.google.com",
                audiences: &[String::from("client")],
                expected_nonce: Some("nonce"),
            },
        )
        .unwrap();
    }

    #[test]
    fn rejects_wrong_audience() {
        let error = validate_claims(
            &claims(),
            OidcValidation {
                issuer: "https://accounts.google.com",
                audiences: &[String::from("other")],
                expected_nonce: Some("nonce"),
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("audience"));
    }
}
