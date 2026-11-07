//! PDF document model: catalog and page tree with inherited attributes
//! (ISO 32000-2:2020, 7.7). Builds on `pdf-syntax`; strict like it (ADR-0002):
//! an inconsistent tree is an error naming the faulty object, not a guess.

mod error;
mod page;

use pdf_syntax::{Budget, Dictionary, Object, ObjectId, PdfFile, Source};

pub use error::{Error, StructureErrorKind};
pub use page::{Page, Rectangle};

/// An opened document: its file structure, catalog and pages in order.
///
/// ```
/// use pdf_document::Document;
/// use pdf_syntax::{Budget, CancellationToken, MemorySource, ResourceLimits};
///
/// let limits = ResourceLimits::default();
/// let bytes = b"%PDF-1.7\n\
///     1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
///     2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 595 842] >>\nendobj\n\
///     3 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n\
///     xref\n0 4\n0000000000 65535 f \n0000000009 00000 n \n\
///     0000000058 00000 n \n0000000139 00000 n \n\
///     trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n186\n%%EOF\n";
/// let source = MemorySource::from_vec(bytes.to_vec(), &limits)?;
/// let document = Document::open(source, &mut Budget::new(limits, CancellationToken::new()))?;
/// assert_eq!(document.page_count(), 1);
/// assert_eq!(document.pages()[0].media_box.width(), 595.0);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct Document<S> {
    file: PdfFile<S>,
    catalog: Dictionary,
    pages: Vec<Page>,
}

/// Structure only: never prints document bytes or object contents.
impl<S: Source> std::fmt::Debug for Document<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("file", &self.file)
            .field("pages", &self.pages.len())
            .finish_non_exhaustive()
    }
}

impl<S: Source> Document<S> {
    /// Opens the file, then reads the catalog and the whole page tree.
    ///
    /// `budget` is charged by the file structure, every object parsed and one
    /// object per page tree node; the tree depth is bounded by
    /// `max_nesting_depth`. Give opening its own budget.
    pub fn open(source: S, budget: &mut Budget) -> Result<Self, Error> {
        let file = PdfFile::open(source, budget)?;
        let root = file
            .root()
            .ok_or(Error::Syntax(pdf_syntax::Error::Malformed {
                offset: file.startxref(),
                kind: pdf_syntax::MalformedKind::InvalidTrailer,
            }))?;
        let catalog_error = Error::InvalidStructure {
            object: root,
            kind: StructureErrorKind::InvalidCatalog,
        };
        let Object::Dictionary(catalog) =
            page::direct(&file, &Object::Reference(root), root, budget)?
        else {
            return Err(catalog_error);
        };
        if catalog.get(b"Type").and_then(Object::as_name) != Some(b"Catalog") {
            return Err(catalog_error);
        }
        let pages_root = catalog.get(b"Pages").and_then(Object::as_reference).ok_or(
            Error::InvalidStructure {
                object: root,
                kind: StructureErrorKind::MissingPages,
            },
        )?;
        let pages = page::collect_pages(&file, pages_root, budget)?;
        Ok(Self {
            file,
            catalog,
            pages,
        })
    }

    /// The underlying file structure, to resolve objects such as resources.
    pub fn file(&self) -> &PdfFile<S> {
        &self.file
    }

    pub fn catalog(&self) -> &Dictionary {
        &self.catalog
    }

    pub fn catalog_id(&self) -> Option<ObjectId> {
        self.file.root()
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Pages in document order.
    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    pub fn page(&self, index: usize) -> Option<&Page> {
        self.pages.get(index)
    }
}

#[cfg(test)]
mod tests;
