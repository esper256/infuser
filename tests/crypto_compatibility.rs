use infuser::{config::Turn, native, peers::Relay, platform, store};

#[test]
fn sha256_keeps_persisted_and_wire_format() -> anyhow::Result<()> {
    // Standard SHA-256 known-answer vectors, independent of the implementation.
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("payload");
    for (input, expected) in [
        (
            "",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            "abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
    ] {
        assert_eq!(platform::hash(input), expected);
        std::fs::write(&path, input)?;
        assert_eq!(native::sha256(&path)?, expected);
    }
    Ok(())
}

#[test]
fn turn_credentials_keep_hmac_sha1_and_padded_base64() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let secret_file = dir.path().join("turn-secret");
    std::fs::write(&secret_file, "0123456789abcdef0123456789abcdef\n")?;
    let relay = Relay::open(&Turn {
        urls: vec!["turn:example.invalid:3478".into()],
        secret_file,
        relay_port_start: 49152,
        relay_port_end: 49252,
        cli_port: None,
        cli_password_file: None,
    })?;
    let credentials = relay.credentials("peer-123", 1000);
    assert_eq!(credentials["username"], "4600:peer-123");
    assert_eq!(credentials["expires_at"], 4600);
    // Independently computed with Python's hmac/hashlib/base64 standard library.
    assert_eq!(credentials["credential"], "mfIgcES3ogsv+jQO2P+P2VzuLG8=");
    Ok(())
}

#[test]
fn generated_secrets_keep_256_bit_encoding() {
    let a = store::secret();
    let b = store::secret();
    for value in [&a, &b] {
        assert_eq!(value.len(), 64);
        assert!(
            value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
    }
    assert_ne!(a, b);
}
