//! TLS root trust for HTTPS downloads of public assets (models, release notes).
//!
//! ureq defaults to its bundled Mozilla root list. Antivirus suites and
//! corporate proxies that inspect HTTPS re-sign traffic with a root they add to
//! the operating system's certificate store, so with the bundled list every
//! model download fails with `invalid peer certificate: UnknownIssuer` (#1256).
//! The platform verifier trusts whatever the OS trusts, which is also what a
//! browser on the same machine accepts.
//!
//! This only changes which roots are trusted. Pinned downloads keep their
//! SHA-256 checks, so an intercepting proxy still cannot substitute a model.

/// TLS settings for downloading public assets: verify against the operating
/// system's certificate store instead of ureq's bundled root list.
pub fn platform_tls_config() -> ureq::tls::TlsConfig {
    ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloads_trust_the_platform_certificate_store() {
        assert!(matches!(
            platform_tls_config().root_certs(),
            ureq::tls::RootCerts::PlatformVerifier
        ));
    }
}
