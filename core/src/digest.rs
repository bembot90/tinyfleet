//! SHA-256, through the sha2 crate.
//!
//! The answer this writes goes onto a record item where it is read back weeks
//! later — so the algorithm has to be one whose output is fixed by its own
//! definition rather than by the toolchain that compiled it. `std::hash`
//! answers neither: its default hasher is explicitly not stable across
//! releases, so a flight opened on one compiler would fail its own
//! verification on the next.

use sha2::Digest;

/// A digest being accumulated over bytes handed to it in pieces.
#[derive(Default)]
pub struct Sha256(sha2::Sha256);

impl Sha256 {
    pub fn new() -> Sha256 {
        Sha256(sha2::Sha256::new())
    }

    pub fn update(&mut self, bytes: &[u8]) {
        Digest::update(&mut self.0, bytes)
    }

    /// The padded tail compressed, and the state rendered as lower-case hex.
    pub fn hex(self) -> String {
        let mut out = String::with_capacity(64);
        for byte in self.0.finalize() {
            out.push_str(&format!("{byte:02x}"));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::Sha256;

    fn hex_of(pieces: &[&[u8]]) -> String {
        let mut digest = Sha256::new();
        for piece in pieces {
            digest.update(piece);
        }
        digest.hex()
    }

    /// The FIPS 180-2 Appendix B and NIST example messages hash to their published digests.
    #[test]
    fn the_published_vectors_hash_to_their_published_digests() {
        assert_eq!(
            hex_of(&[b""]),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex_of(&[b"abc"]),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex_of(&[b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"]),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    /// Messages of every length around a block and a padding boundary hash as `shasum -a 256` does.
    #[test]
    fn every_padding_edge_hashes_as_the_standard_says() {
        let edges = [
            (
                55,
                "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318",
            ),
            (
                56,
                "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a",
            ),
            (
                63,
                "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34",
            ),
            (
                64,
                "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb",
            ),
            (
                65,
                "635361c48bb9eab14198e76ea8ab7f1a41685d6ad62aa9146d301d4f17eb0ae0",
            ),
            (
                119,
                "31eba51c313a5c08226adf18d4a359cfdfd8d2e816b13f4af952f7ea6584dcfb",
            ),
            (
                120,
                "2f3d335432c70b580af0e8e1b3674a7c020d683aa5f73aaaedfdc55af904c21c",
            ),
        ];
        for (n, expected) in edges {
            assert_eq!(hex_of(&[&vec![b'a'; n]]), expected, "{n} bytes of 'a'");
        }
    }

    /// A message fed in pieces that straddle every block boundary hashes as the same message whole.
    #[test]
    fn a_message_handed_over_in_pieces_hashes_as_it_does_whole() {
        let mut digest = Sha256::new();
        let million = vec![b'a'; 1_000_000];
        for piece in million.chunks(7) {
            digest.update(piece);
        }
        assert_eq!(
            digest.hex(),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
        assert_eq!(hex_of(&[b"a", b"bc"]), hex_of(&[b"abc"]));
    }
}
