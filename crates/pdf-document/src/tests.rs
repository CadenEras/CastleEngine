use pdf_syntax::{CancellationToken, LimitKind, MemorySource, ResourceLimits};

use super::*;

/// Builds a valid classic-xref file: object n is `bodies[n - 1]`, object 1 the
/// catalog.
fn build(bodies: &[&str]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    let size = bodies.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

fn open_with(bodies: &[&str], budget: &mut Budget) -> Result<Document<MemorySource>, Error> {
    let source = MemorySource::from_vec(build(bodies), &ResourceLimits::default())?;
    Document::open(source, budget)
}

fn open(bodies: &[&str]) -> Result<Document<MemorySource>, Error> {
    open_with(
        bodies,
        &mut Budget::new(ResourceLimits::default(), CancellationToken::new()),
    )
}

fn id(number: u32) -> ObjectId {
    ObjectId {
        number,
        generation: 0,
    }
}

fn structure(number: u32, kind: StructureErrorKind) -> Result<(), Error> {
    Err(Error::InvalidStructure {
        object: id(number),
        kind,
    })
}

fn outcome(result: Result<Document<MemorySource>, Error>) -> Result<(), Error> {
    result.map(|_| ())
}

const CATALOG: &str = "<< /Type /Catalog /Pages 2 0 R >>";
const A4: &str = "/MediaBox [0 0 595 842]";

fn rect(llx: f64, lly: f64, urx: f64, ury: f64) -> Rectangle {
    Rectangle::new(llx, lly, urx, ury)
}

#[test]
fn single_page() {
    let doc = open(&[
        CATALOG,
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>",
        "<< /Length 0 >>\nstream\n\nendstream",
    ])
    .expect("valid document");
    assert_eq!(doc.page_count(), 1);
    assert_eq!(doc.catalog_id(), Some(id(1)));
    let page = doc.page(0).expect("page");
    assert_eq!(page.id, id(3));
    assert_eq!(page.media_box, rect(0.0, 0.0, 612.0, 792.0));
    assert_eq!(page.crop_box, page.media_box);
    assert_eq!(page.rotation, 0);
    assert_eq!(page.resources, None);
    assert_eq!(page.contents, Some(Object::Reference(id(4))));
    assert!(doc.page(1).is_none());
}

#[test]
fn nested_tree_keeps_document_order_and_inherits_attributes() {
    let doc = open(&[
        CATALOG,
        // Root: MediaBox, Rotate and Resources for everyone.
        "<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 3 /MediaBox [0 0 595 842] \
         /Rotate -90 /Resources << /Font << >> >> >>",
        // Intermediate node: a CropBox larger than the MediaBox on one side.
        "<< /Type /Pages /Parent 2 0 R /Kids [4 0 R 5 0 R] /Count 2 \
         /CropBox [10 10 700 800] >>",
        "<< /Type /Page /Parent 3 0 R >>",
        // Overrides: its own MediaBox (corners in any order) and Rotate.
        "<< /Type /Page /Parent 3 0 R /MediaBox [200 300 0 0] /Rotate 450 >>",
        "<< /Type /Page /Parent 2 0 R /Resources 7 0 R >>",
        "<< /ProcSet [/PDF] >>",
    ])
    .expect("valid document");
    let ids: Vec<u32> = doc.pages().iter().map(|p| p.id.number).collect();
    assert_eq!(ids, vec![4, 5, 6]);
    let [first, second, third] = doc.pages() else {
        panic!("three pages");
    };
    assert_eq!(first.media_box, rect(0.0, 0.0, 595.0, 842.0));
    assert_eq!(first.crop_box, rect(10.0, 10.0, 595.0, 800.0));
    assert_eq!(first.rotation, 270);
    assert!(matches!(
        first.resources.as_deref(),
        Some(Object::Dictionary(_))
    ));
    assert_eq!(second.media_box, rect(0.0, 0.0, 200.0, 300.0));
    assert_eq!(second.crop_box, rect(10.0, 10.0, 200.0, 300.0));
    assert_eq!(second.rotation, 90);
    assert_eq!(third.crop_box, third.media_box);
    assert_eq!(third.resources.as_deref(), Some(&Object::Reference(id(7))));
}

#[test]
fn indirect_values_are_resolved_once() {
    let doc = open(&[
        CATALOG,
        "<< /Type /Pages /Kids 4 0 R /Count 1 /MediaBox 5 0 R /Rotate 6 0 R >>",
        "<< /Type /Page /Parent 2 0 R >>",
        "[3 0 R]",
        "[0 0 100.5 200]",
        "180",
    ])
    .expect("valid document");
    let page = doc.page(0).expect("page");
    assert_eq!(page.media_box, rect(0.0, 0.0, 100.5, 200.0));
    assert_eq!(page.rotation, 180);
    // A reference to a reference, or to a stream, is refused.
    assert_eq!(
        outcome(open(&[
            CATALOG,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox 4 0 R >>",
            "<< /Type /Page /Parent 2 0 R >>",
            "5 0 R",
            "[0 0 1 1]",
        ])),
        structure(2, StructureErrorKind::UnexpectedIndirection)
    );
}

#[test]
fn empty_document() {
    let doc = open(&[CATALOG, "<< /Type /Pages /Kids [] /Count 0 >>"]).expect("valid");
    assert_eq!(doc.page_count(), 0);
}

#[test]
fn invalid_catalogs() {
    assert_eq!(
        outcome(open(&["<< /Type /Pages /Kids [] /Count 0 >>"])),
        structure(1, StructureErrorKind::InvalidCatalog)
    );
    assert_eq!(
        outcome(open(&["(not a dictionary)"])),
        structure(1, StructureErrorKind::InvalidCatalog)
    );
    assert_eq!(
        outcome(open(&["<< /Type /Catalog >>"])),
        structure(1, StructureErrorKind::MissingPages)
    );
    assert_eq!(
        outcome(open(&["<< /Type /Catalog /Pages << >> >>"])),
        structure(1, StructureErrorKind::MissingPages)
    );
}

#[test]
fn invalid_nodes() {
    let pages = |root: &'static str, page: &'static str| outcome(open(&[CATALOG, root, page]));
    // The root must be a /Pages node, and leaves /Page.
    assert_eq!(
        pages("<< /Type /Page /MediaBox [0 0 1 1] >>", "null"),
        structure(2, StructureErrorKind::InvalidNode)
    );
    assert_eq!(
        pages(
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Font /Parent 2 0 R >>"
        ),
        structure(3, StructureErrorKind::InvalidNode)
    );
    assert_eq!(
        pages(
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Parent 2 0 R >>"
        ),
        structure(3, StructureErrorKind::InvalidNode)
    );
    // A kid that is free or absent is reported on the node listing it.
    assert_eq!(
        pages("<< /Type /Pages /Kids [9 0 R] /Count 1 >>", "null"),
        structure(2, StructureErrorKind::MissingKid)
    );
    assert_eq!(
        pages(
            "<< /Type /Pages /Kids [<< /Type /Page >>] /Count 1 >>",
            "null"
        ),
        structure(2, StructureErrorKind::InvalidKids)
    );
    assert_eq!(
        pages("<< /Type /Pages /Count 0 >>", "null"),
        structure(2, StructureErrorKind::InvalidKids)
    );
}

#[test]
fn counts_and_parents_must_be_consistent() {
    let page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1 1] >>";
    assert_eq!(
        outcome(open(&[
            CATALOG,
            "<< /Type /Pages /Kids [3 0 R] /Count 2 >>",
            page
        ])),
        structure(2, StructureErrorKind::CountMismatch)
    );
    assert_eq!(
        outcome(open(&[CATALOG, "<< /Type /Pages /Kids [3 0 R] >>", page])),
        structure(2, StructureErrorKind::CountMismatch)
    );
    assert_eq!(
        outcome(open(&[
            CATALOG,
            "<< /Type /Pages /Kids [3 0 R] /Count -1 >>",
            page
        ])),
        structure(2, StructureErrorKind::CountMismatch)
    );
    assert_eq!(
        outcome(open(&[
            CATALOG,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 1 0 R /MediaBox [0 0 1 1] >>",
        ])),
        structure(3, StructureErrorKind::InvalidParent)
    );
    assert_eq!(
        outcome(open(&[
            CATALOG,
            "<< /Type /Pages /Parent 1 0 R /Kids [3 0 R] /Count 1 >>",
            page,
        ])),
        structure(2, StructureErrorKind::InvalidParent)
    );
}

#[test]
fn cycles_and_shared_nodes_are_refused() {
    // A node listing its own ancestor.
    assert_eq!(
        outcome(open(&[
            CATALOG,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Pages /Parent 2 0 R /Kids [2 0 R] /Count 1 >>",
        ])),
        structure(2, StructureErrorKind::RepeatedNode)
    );
    // The same page listed twice.
    assert_eq!(
        outcome(open(&[
            CATALOG,
            "<< /Type /Pages /Kids [3 0 R 3 0 R] /Count 2 /MediaBox [0 0 1 1] >>",
            "<< /Type /Page /Parent 2 0 R >>",
        ])),
        structure(3, StructureErrorKind::RepeatedNode)
    );
}

#[test]
fn boxes_and_rotation_are_validated() {
    let with = |attributes: &str| {
        let root = "<< /Type /Pages /Kids [3 0 R] /Count 1 >>";
        let page = format!("<< /Type /Page /Parent 2 0 R {attributes} >>");
        outcome(open(&[CATALOG, root, page.as_str()]))
    };
    assert_eq!(with(""), structure(3, StructureErrorKind::InvalidBox));
    assert_eq!(
        with("/MediaBox [0 0 1]"),
        structure(3, StructureErrorKind::InvalidBox)
    );
    assert_eq!(
        with("/MediaBox [0 0 1 (x)]"),
        structure(3, StructureErrorKind::InvalidBox)
    );
    assert_eq!(
        with(&format!("{A4} /CropBox [0 0 1]")),
        structure(3, StructureErrorKind::InvalidBox)
    );
    assert_eq!(
        with(&format!("{A4} /Rotate 45")),
        structure(3, StructureErrorKind::InvalidRotate)
    );
    assert_eq!(
        with(&format!("{A4} /Rotate 90.0")),
        structure(3, StructureErrorKind::InvalidRotate)
    );
    assert_eq!(with(&format!("{A4} /Rotate -720")), Ok(()));
}

#[test]
fn tree_depth_exact_limit_and_limit_plus_one() {
    // Root (depth 1) -> 2 intermediate nodes -> page at depth 4.
    let bodies = [
        CATALOG,
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 1 1] >>",
        "<< /Type /Pages /Parent 2 0 R /Kids [4 0 R] /Count 1 >>",
        "<< /Type /Pages /Parent 3 0 R /Kids [5 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 4 0 R >>",
    ];
    let limited = |max_nesting_depth| {
        // ResourceLimits is non-exhaustive outside pdf-syntax.
        let mut limits = ResourceLimits::default();
        limits.max_nesting_depth = max_nesting_depth;
        Budget::new(limits, CancellationToken::new())
    };
    assert!(open_with(&bodies, &mut limited(4)).is_ok());
    assert_eq!(
        outcome(open_with(&bodies, &mut limited(3))),
        Err(Error::Syntax(pdf_syntax::Error::LimitExceeded {
            kind: LimitKind::NestingDepth,
            limit: 3
        }))
    );
}

#[test]
fn cancellation_stops_opening() {
    let token = CancellationToken::new();
    token.cancel();
    let mut budget = Budget::new(ResourceLimits::default(), token);
    assert_eq!(
        outcome(open_with(&[CATALOG], &mut budget)),
        Err(Error::Syntax(pdf_syntax::Error::Cancelled))
    );
}

#[test]
fn errors_display_the_object() {
    let error = Error::InvalidStructure {
        object: id(12),
        kind: StructureErrorKind::CountMismatch,
    };
    assert_eq!(
        error.to_string(),
        "invalid document structure at object 12 0: CountMismatch"
    );
}

#[test]
fn null_values_and_free_references_count_as_absent() {
    let root = "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 10 20] /Rotate 90 \
                /Resources << /Font << >> >> /Parent null >>";
    // Every inheritable key set to null, or to a free object (9), on the page.
    for value in ["null", "9 0 R"] {
        let page = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox {value} /CropBox {value} \
             /Rotate {value} /Resources {value} /Contents {value} >>"
        );
        let doc = open(&[CATALOG, root, page.as_str()]).expect(value);
        let page = doc.page(0).expect("page");
        assert_eq!(page.media_box, rect(0.0, 0.0, 10.0, 20.0), "{value}");
        assert_eq!(page.crop_box, page.media_box);
        assert_eq!(page.rotation, 90);
        assert!(matches!(
            page.resources.as_deref(),
            Some(Object::Dictionary(_))
        ));
        // /Contents is not inherited: kept as written unless null.
        if value == "null" {
            assert_eq!(page.contents, None);
        }
    }
}

#[test]
fn wide_trees_share_inherited_resources() {
    // 2 000 pages under one node with large inherited resources: every page
    // shares the same allocation instead of a copy.
    let big = "x".repeat(100_000);
    let kids: String = (0..2000).map(|i| format!("{} 0 R ", i + 3)).collect();
    let root = format!(
        "<< /Type /Pages /Kids [{kids}] /Count 2000 /MediaBox [0 0 1 1] \
         /Resources << /Properties << /P ({big}) >> >> >>"
    );
    let page = "<< /Type /Page /Parent 2 0 R >>";
    let mut bodies = vec![CATALOG, root.as_str()];
    bodies.extend(std::iter::repeat_n(page, 2000));
    let doc = open(&bodies).expect("valid wide tree");
    assert_eq!(doc.page_count(), 2000);
    let first = doc
        .page(0)
        .and_then(|p| p.resources.clone())
        .expect("resources");
    assert!(doc.pages().iter().all(|p| {
        p.resources
            .as_ref()
            .is_some_and(|r| std::sync::Arc::ptr_eq(r, &first))
    }));
}

#[test]
fn inherited_attribute_errors_name_the_carrying_node() {
    let page = "<< /Type /Page /Parent 2 0 R >>";
    for (attribute, kind) in [
        ("/MediaBox [0 0 1]", StructureErrorKind::InvalidBox),
        (
            "/MediaBox [0 0 1 1] /CropBox (x)",
            StructureErrorKind::InvalidBox,
        ),
        (
            "/MediaBox [0 0 1 1] /Rotate 45",
            StructureErrorKind::InvalidRotate,
        ),
        (
            "/MediaBox [0 0 1 1] /Resources 7",
            StructureErrorKind::InvalidResources,
        ),
    ] {
        let root = format!("<< /Type /Pages /Kids [3 0 R] /Count 1 {attribute} >>");
        assert_eq!(
            outcome(open(&[CATALOG, root.as_str(), page])),
            structure(2, kind),
            "{attribute}"
        );
    }
}

#[test]
fn box_and_rotation_edge_values() {
    let with = |attributes: &str| {
        let root = "<< /Type /Pages /Kids [3 0 R] /Count 1 >>";
        let page = format!("<< /Type /Page /Parent 2 0 R {attributes} >>");
        open(&[CATALOG, root, page.as_str()]).map(|d| d.pages().to_vec())
    };
    // A CropBox disjoint from the MediaBox gives an empty crop box.
    let pages = with("/MediaBox [0 0 10 10] /CropBox [20 20 30 30]").expect("valid");
    let crop = pages.first().map(|p| p.crop_box).expect("page");
    assert_eq!((crop.width(), crop.height()), (0.0, 0.0));
    // Extreme rotations: multiples of 90 near the i64 range end.
    let max_multiple = i64::MAX - i64::MAX % 90;
    let min_multiple = i64::MIN - i64::MIN % 90;
    for degrees in [max_multiple, min_multiple] {
        let pages = with(&format!("{A4} /Rotate {degrees}")).expect("valid");
        let rotation = pages.first().map(|p| p.rotation).expect("page");
        assert_eq!(i64::from(rotation), degrees.rem_euclid(360));
    }
    assert!(with(&format!("{A4} /Rotate {}", i64::MAX)).is_err());
    // Finite but extreme coordinates (PDF reals have no exponent) are
    // accepted; the width overflows to infinity, as documented on Rectangle.
    let huge = format!("17{}.0", "0".repeat(307));
    let pages = with(&format!("/MediaBox [-{huge} 0 {huge} 1]")).expect("valid");
    let media = pages.first().map(|p| p.media_box).expect("page");
    assert!(media.llx.is_finite() && media.urx.is_finite());
    assert!(media.width().is_infinite());
}

#[test]
fn kids_pointing_at_the_catalog_or_another_generation() {
    assert_eq!(
        outcome(open(&[
            CATALOG,
            "<< /Type /Pages /Kids [1 0 R] /Count 0 >>"
        ])),
        structure(1, StructureErrorKind::InvalidParent)
    );
    // Generation 1 does not match the entry (generation 0): a free reference.
    assert_eq!(
        outcome(open(&[
            CATALOG,
            "<< /Type /Pages /Kids [3 1 R] /Count 1 /MediaBox [0 0 1 1] >>",
            "<< /Type /Page /Parent 2 0 R >>",
        ])),
        structure(2, StructureErrorKind::MissingKid)
    );
}

#[test]
fn tree_object_budget_exact_limit_and_limit_plus_one() {
    let bodies = [
        CATALOG,
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 1 1] >>",
        "<< /Type /Page /Parent 2 0 R >>",
    ];
    let bytes = build(&bodies);
    let mut probe = Budget::new(ResourceLimits::default(), CancellationToken::new());
    let source = MemorySource::from_vec(bytes.clone(), &ResourceLimits::default()).expect("ok");
    assert!(Document::open(source, &mut probe).is_ok());
    let needed = probe.objects_visited();
    let limited = |max| {
        let mut limits = ResourceLimits::default();
        limits.max_objects_visited = max;
        Budget::new(limits, CancellationToken::new())
    };
    let open_limited = |max| {
        let source = MemorySource::from_vec(bytes.clone(), &ResourceLimits::default())?;
        Document::open(source, &mut limited(max)).map(|_| ())
    };
    assert_eq!(open_limited(needed), Ok(()));
    assert_eq!(
        open_limited(needed - 1),
        Err(Error::Syntax(pdf_syntax::Error::LimitExceeded {
            kind: LimitKind::ObjectsVisited,
            limit: needed - 1
        }))
    );
}

#[test]
fn cancellation_stops_the_page_tree_walk() {
    // The file opens; the walk itself then sees the cancelled token.
    let bytes = build(&[
        CATALOG,
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 1 1] >>",
        "<< /Type /Page /Parent 2 0 R >>",
    ]);
    let source = MemorySource::from_vec(bytes, &ResourceLimits::default()).expect("ok");
    let file = PdfFile::open(
        source,
        &mut Budget::new(ResourceLimits::default(), CancellationToken::new()),
    )
    .expect("valid file");
    let token = CancellationToken::new();
    token.cancel();
    let mut budget = Budget::new(ResourceLimits::default(), token);
    assert_eq!(
        page::collect_pages(&file, id(2), &mut budget),
        Err(Error::Syntax(pdf_syntax::Error::Cancelled))
    );
}

#[test]
fn page_debug_never_prints_document_data() {
    let doc = open(&[
        CATALOG,
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 1 1] \
         /Resources << /Secret (private text) >> >>",
        "<< /Type /Page /Parent 2 0 R >>",
    ])
    .expect("valid");
    let printed = format!("{:?} {doc:?}", doc.pages());
    assert!(!printed.contains("private"), "{printed}");
}
