//! Signed Bearer tokens used by the native development server.
//!
//! Tokens intentionally carry only a subject and an expiry. Application data
//! remains in its own store or database instead of being copied into a token.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::Digest;
use sha2::Sha256;

use crate::runtime::AuthConfig;

type HmacSha256 = Hmac<Sha256>;
static TOKEN_NONCE: AtomicU64 = AtomicU64::new(0);

/// Why a supplied authorization value cannot authenticate a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenError {
    Missing,
    Invalid,
    Expired,
}

/// The capability carried by a signed token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenKind {
    Access,
    Refresh,
}

/// A token subject only after its signature and expiry have been checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthenticatedToken {
    pub(crate) subject: String,
    pub(crate) kind: TokenKind,
}

/// Creates one URL-safe, HMAC-SHA256 signed token for an application subject.
pub(crate) fn issue(configuration: &AuthConfig, subject: &str) -> String {
    issue_with_kind(configuration, subject, TokenKind::Access)
}

/// Creates a longer-lived refresh token that can only be exchanged for an access token.
pub(crate) fn issue_refresh(configuration: &AuthConfig, subject: &str) -> String {
    issue_with_kind(configuration, subject, TokenKind::Refresh)
}

fn issue_with_kind(configuration: &AuthConfig, subject: &str, kind: TokenKind) -> String {
    let lifetime = match kind {
        TokenKind::Access => configuration.access_duration_milliseconds,
        TokenKind::Refresh => configuration.refresh_duration_milliseconds,
    };
    let expires_at = now_milliseconds().saturating_add(lifetime);
    let subject = URL_SAFE_NO_PAD.encode(subject.as_bytes());
    let kind = match kind {
        TokenKind::Access => "access",
        TokenKind::Refresh => "refresh",
    };
    // The nonce makes concurrently issued sessions distinct even within one clock tick.
    let nonce = TOKEN_NONCE.fetch_add(1, Ordering::Relaxed);
    let payload = format!("orni1.{kind}.{subject}.{expires_at}.{nonce}");
    let signature = signature(&configuration.secret, &payload);
    format!("{payload}.{signature}")
}

/// Validates a Bearer token and returns its subject only after its signature and expiry pass.
pub(crate) fn validate(
    configuration: &AuthConfig,
    authorization: Option<&str>,
) -> Result<AuthenticatedToken, TokenError> {
    let authorization = authorization.ok_or(TokenError::Missing)?;
    let Some((scheme, token)) = authorization.split_once(' ') else {
        return Err(TokenError::Invalid);
    };
    if !scheme.eq_ignore_ascii_case("Bearer")
        || token.is_empty()
        || token.contains(char::is_whitespace)
    {
        return Err(TokenError::Invalid);
    }
    let mut parts = token.split('.');
    let (
        Some(version),
        Some(kind),
        Some(encoded_subject),
        Some(expires_at),
        Some(nonce),
        Some(encoded_signature),
    ) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    )
    else {
        return Err(TokenError::Invalid);
    };
    if parts.next().is_some() || version != "orni1" {
        return Err(TokenError::Invalid);
    }
    let kind = match kind {
        "access" => TokenKind::Access,
        "refresh" => TokenKind::Refresh,
        _ => return Err(TokenError::Invalid),
    };
    if nonce.parse::<u64>().is_err() {
        return Err(TokenError::Invalid);
    }
    let payload = format!(
        "{version}.{}.{encoded_subject}.{expires_at}.{nonce}",
        match kind {
            TokenKind::Access => "access",
            TokenKind::Refresh => "refresh",
        }
    );
    let supplied = URL_SAFE_NO_PAD
        .decode(encoded_signature)
        .map_err(|_| TokenError::Invalid)?;
    let mut mac = HmacSha256::new_from_slice(configuration.secret.as_bytes())
        .map_err(|_| TokenError::Invalid)?;
    mac.update(payload.as_bytes());
    mac.verify_slice(&supplied)
        .map_err(|_| TokenError::Invalid)?;
    let expires_at = expires_at.parse::<u64>().map_err(|_| TokenError::Invalid)?;
    if expires_at <= now_milliseconds() {
        return Err(TokenError::Expired);
    }
    let subject = URL_SAFE_NO_PAD
        .decode(encoded_subject)
        .map_err(|_| TokenError::Invalid)?;
    let subject = String::from_utf8(subject).map_err(|_| TokenError::Invalid)?;
    if subject.is_empty() || subject.len() > 256 {
        return Err(TokenError::Invalid);
    }
    Ok(AuthenticatedToken { subject, kind })
}

fn signature(secret: &str, payload: &str) -> String {
    URL_SAFE_NO_PAD.encode(signature_bytes(secret, payload))
}

fn signature_bytes(secret: &str, payload: &str) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts secrets of any length");
    mac.update(payload.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn now_milliseconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

/// Produces a non-reversible identifier for short-lived in-memory refresh tracking.
pub(crate) fn fingerprint(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::{AuthConfig, TokenError, TokenKind, issue, validate};

    fn configuration() -> AuthConfig {
        AuthConfig {
            secret: "a development secret with enough entropy".to_owned(),
            access_duration_milliseconds: 3_600_000,
            refresh_duration_milliseconds: 604_800_000,
            maximum_sessions_per_subject: 2,
        }
    }

    #[test]
    fn accepts_a_token_issued_by_the_same_application() {
        let configuration = configuration();
        let token = issue(&configuration, "user-42");
        assert_eq!(
            validate(&configuration, Some(&format!("Bearer {token}"))),
            Ok(super::AuthenticatedToken {
                subject: "user-42".to_owned(),
                kind: TokenKind::Access
            })
        );
    }

    #[test]
    fn rejects_tampered_or_missing_tokens() {
        let configuration = configuration();
        let token = issue(&configuration, "user-42");
        assert_eq!(validate(&configuration, None), Err(TokenError::Missing));
        assert_eq!(
            validate(&configuration, Some(&format!("Bearer {token}x"))),
            Err(TokenError::Invalid)
        );
    }
}
