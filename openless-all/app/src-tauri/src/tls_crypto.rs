pub(crate) fn install_tls_crypto_provider() {
    // Select the process provider before any HTTP or WebSocket client is built,
    // including on mobile. Repeated installation leaves the existing default intact.
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
mod tests {
    #[test]
    fn tls_provider_install_allows_client_config() {
        super::install_tls_crypto_provider();
        super::install_tls_crypto_provider();
        let _ = rustls::ClientConfig::builder()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();
    }
}
