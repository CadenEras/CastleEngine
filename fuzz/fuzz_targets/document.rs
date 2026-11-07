//! Opens arbitrary bytes as a document: file structure, catalog and page tree.
//! Properties: no panic, overflow or unbounded work; cycles and shared nodes
//! are refused; every returned page has normalised, finite boxes and a
//! rotation in {0, 90, 180, 270}.
#![no_main]

use std::collections::BTreeSet;

use libfuzzer_sys::fuzz_target;
use pdf_document::Document;
use pdf_syntax::{Budget, CancellationToken, MemorySource, ResourceLimits};

fuzz_target!(|data: &[u8]| {
    let mut limits = ResourceLimits::default();
    limits.max_decoded_bytes = 1 << 20;
    limits.max_objects_visited = 100_000;
    limits.max_nesting_depth = 32;
    let Ok(source) = MemorySource::from_vec(data.to_vec(), &limits) else {
        return;
    };
    let Ok(doc) = Document::open(source, &mut Budget::new(limits, CancellationToken::new())) else {
        return;
    };
    let mut seen = BTreeSet::new();
    for page in doc.pages() {
        assert!(seen.insert(page.id), "page listed twice");
        for b in [page.media_box, page.crop_box] {
            assert!(b.llx.is_finite() && b.lly.is_finite() && b.urx.is_finite() && b.ury.is_finite());
            assert!(b.llx <= b.urx && b.lly <= b.ury);
        }
        assert!(matches!(page.rotation, 0 | 90 | 180 | 270));
    }
});
