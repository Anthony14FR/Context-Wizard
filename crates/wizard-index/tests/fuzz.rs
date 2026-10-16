//! The cached structure is read back from the index, so corrupt bytes must be rejected cleanly.

use proptest::prelude::{any, proptest};
use wizard_graph::Graph;
use wizard_index::structure::Structure;

proptest! {
    #[test]
    fn any_bytes_decode_or_are_rejected(bytes in any::<Vec<u8>>()) {
        let _ = Structure::from_bytes(&bytes);
        let _ = Graph::from_bytes(&bytes);
    }
}
