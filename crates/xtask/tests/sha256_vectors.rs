//! SHA-256 vectors for the pure-Rust digest (NIST + oracle-derived).

#[test]
fn nist_vectors() {
    assert_eq!(
        xtask::sha256::hexdigest(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        xtask::sha256::hexdigest(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        xtask::sha256::hexdigest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

#[test]
fn multiblock_vector() {
    // 71 bytes: spans two blocks; expected value from system shasum.
    let input = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopqrstuvwxyz012345";
    assert_eq!(input.len(), 71);
    assert_eq!(
        xtask::sha256::hexdigest(input),
        "780d3c2718397e897e5004c4af6936c78e7100250c0725861d4a1f46a1b78142"
    );
}
