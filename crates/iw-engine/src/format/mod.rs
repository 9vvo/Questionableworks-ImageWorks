//! File formats: the native document format, and PNG and JPEG.
//!
//! Each format is its own module with a reader and a writer that work on
//! in-memory bytes or streams. The functions here add the file-system side:
//! picking a format, and saving atomically.

pub mod jpeg;
pub mod native;
pub mod png;

use crate::document::{Document, DocumentError};
use crate::io::atomic_write;
use std::fmt;
use std::io::{BufWriter, Write};
use std::path::Path;

/// Largest image, in pixels, an importer will decode. Guards against
/// files that claim absurd dimensions.
pub const MAX_IMPORT_PIXELS: u64 = 1 << 30;

/// Why a file could not be read or written.
#[derive(Debug)]
pub enum FormatError {
    Io(std::io::Error),
    /// The file is damaged or is not what it claims to be.
    Malformed(String),
    /// The file is valid but uses something this version cannot handle.
    Unsupported(String),
    /// A native document written by a newer version of the application.
    TooNew {
        version: u32,
        supported: u32,
    },
    /// The file describes a document the engine rejects.
    Document(DocumentError),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Malformed(why) => write!(f, "the file is damaged or not a valid image: {why}"),
            Self::Unsupported(what) => write!(f, "not supported: {what}"),
            Self::TooNew { version, supported } => write!(
                f,
                "the document was saved by a newer version (format {version}; this version reads up to {supported})"
            ),
            Self::Document(e) => write!(f, "the file describes an invalid document: {e}"),
        }
    }
}

impl std::error::Error for FormatError {}

impl From<std::io::Error> for FormatError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<DocumentError> for FormatError {
    fn from(e: DocumentError) -> Self {
        Self::Document(e)
    }
}

pub(crate) fn malformed(why: impl Into<String>) -> FormatError {
    FormatError::Malformed(why.into())
}

/// A file format the engine can read and write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileFormat {
    /// The native layered document.
    Native,
    Png,
    Jpeg,
}

impl FileFormat {
    pub const ALL: [FileFormat; 3] = [FileFormat::Native, FileFormat::Png, FileFormat::Jpeg];

    /// File extensions, lower case, preferred one first.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            FileFormat::Native => &[native::EXTENSION],
            FileFormat::Png => &["png"],
            FileFormat::Jpeg => &["jpg", "jpeg", "jpe"],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FileFormat::Native => "ImageWorks Document",
            FileFormat::Png => "PNG",
            FileFormat::Jpeg => "JPEG",
        }
    }

    /// Whether saving in this format keeps layers and everything else.
    pub fn preserves_document(self) -> bool {
        self == FileFormat::Native
    }

    /// The format a path's extension names.
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|f| f.extensions().contains(&ext.as_str()))
    }

    /// The format the first bytes of a file identify.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            Some(FileFormat::Png)
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(FileFormat::Jpeg)
        } else if bytes.starts_with(b"PK\x03\x04") {
            Some(FileFormat::Native)
        } else {
            None
        }
    }
}

/// Settings for exporting to a flat image format.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExportOptions {
    /// JPEG quality, 1 (smallest) to 100 (best).
    pub jpeg_quality: u8,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { jpeg_quality: 90 }
    }
}

/// Reads a document from bytes. The content decides the format; the file
/// name is not consulted, so a mis-named file still opens.
pub fn read(bytes: &[u8]) -> Result<Document, FormatError> {
    match FileFormat::sniff(bytes) {
        Some(FileFormat::Native) => native::read(std::io::Cursor::new(bytes)),
        Some(FileFormat::Png) => png::read(bytes),
        Some(FileFormat::Jpeg) => jpeg::read(bytes),
        None => Err(FormatError::Unsupported("unrecognised file type".into())),
    }
}

/// Opens a file as a document.
pub fn open(path: &Path) -> Result<Document, FormatError> {
    read(&std::fs::read(path)?)
}

/// Writes `document` to `path` in `format`, atomically: an existing file at
/// `path` is replaced only once the new one is completely written.
pub fn save(
    document: &Document,
    path: &Path,
    format: FileFormat,
    options: &ExportOptions,
) -> Result<(), FormatError> {
    atomic_write(path, |file| {
        let mut out = BufWriter::new(file);
        match format {
            FileFormat::Native => native::write(document, &mut out)?,
            FileFormat::Png => png::write(document, &mut out)?,
            FileFormat::Jpeg => jpeg::write(document, &mut out, options.jpeg_quality)?,
        }
        out.flush()?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_are_found_by_extension_and_by_content() {
        assert_eq!(
            FileFormat::from_path(Path::new("a/b/Photo.JPG")),
            Some(FileFormat::Jpeg)
        );
        assert_eq!(
            FileFormat::from_path(Path::new("x.jpeg")),
            Some(FileFormat::Jpeg)
        );
        assert_eq!(
            FileFormat::from_path(Path::new("x.png")),
            Some(FileFormat::Png)
        );
        assert_eq!(
            FileFormat::from_path(Path::new("x.iwdoc")),
            Some(FileFormat::Native)
        );
        assert_eq!(FileFormat::from_path(Path::new("x.psd")), None);
        assert_eq!(FileFormat::from_path(Path::new("noext")), None);

        assert_eq!(
            FileFormat::sniff(b"\x89PNG\r\n\x1a\n...."),
            Some(FileFormat::Png)
        );
        assert_eq!(
            FileFormat::sniff(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(FileFormat::Jpeg)
        );
        assert_eq!(FileFormat::sniff(b"PK\x03\x04"), Some(FileFormat::Native));
        assert_eq!(FileFormat::sniff(b"GIF89a"), None);
        assert_eq!(FileFormat::sniff(b""), None);
    }

    #[test]
    fn unrecognised_bytes_are_an_error_not_a_panic() {
        assert!(matches!(
            read(b"this is not an image"),
            Err(FormatError::Unsupported(_))
        ));
        assert!(read(b"").is_err());
    }

    use crate::command::Command;
    use crate::document::{Layer, PixelData};
    use crate::history::Session;
    use crate::io::test_support::TempDir;
    use crate::pixel::{BitDepth, ByDepth};
    use crate::raster::Raster;
    use crate::tile::{Tile, TileCoord};

    fn sample(seed: u8) -> Document {
        let mut s = Session::new(Document::new(300, 200, BitDepth::U8).unwrap());
        for (i, name) in ["Bottom", "Top"].into_iter().enumerate() {
            let mut r: Raster<u8> = Raster::new();
            let v = seed.wrapping_add(i as u8 * 60);
            r.insert_tile(TileCoord::new(0, 0), Tile::filled([v, v / 2, 10, 255]));
            let layer = Layer::raster(name, PixelData::U8(r));
            s.execute(Command::AddLayer {
                parent: None,
                index: i,
                layer: Box::new(layer),
            })
            .unwrap();
        }
        s.into_document()
    }

    #[test]
    fn native_files_save_and_open() {
        let dir = TempDir::new("format-native");
        let path = dir.file("doc.iwdoc");
        let doc = sample(100);
        save(&doc, &path, FileFormat::Native, &ExportOptions::default()).unwrap();
        assert!(open(&path).unwrap() == doc);

        // Saving again replaces the file and leaves nothing else behind.
        let changed = sample(7);
        save(
            &changed,
            &path,
            FileFormat::Native,
            &ExportOptions::default(),
        )
        .unwrap();
        assert!(open(&path).unwrap() == changed);
        assert_eq!(dir.entries(), ["doc.iwdoc"]);
    }

    #[test]
    fn flat_exports_open_as_one_layer() {
        let dir = TempDir::new("format-flat");
        let doc = sample(100);
        for (name, format) in [("out.png", FileFormat::Png), ("out.jpg", FileFormat::Jpeg)] {
            let path = dir.file(name);
            save(&doc, &path, format, &ExportOptions::default()).unwrap();
            let back = open(&path).unwrap();
            assert_eq!((back.width(), back.height()), (300, 200));
            assert_eq!(back.layers().len(), 1, "{name} should flatten to one layer");
            let ByDepth::U8(pixels) = back.flatten().pixels else {
                panic!("expected 8-bit")
            };
            // The top layer (160, 80, 10) covers the tile; JPEG is approximate.
            let p = pixels[0];
            assert!(
                p[0].abs_diff(160) <= 3 && p[1].abs_diff(80) <= 3 && p[2].abs_diff(10) <= 3,
                "{name}: {p:?}"
            );
        }
    }

    #[test]
    fn opening_goes_by_content_not_by_file_name() {
        let dir = TempDir::new("format-sniff");
        let path = dir.file("actually-a-png.jpg");
        save(
            &sample(100),
            &path,
            FileFormat::Png,
            &ExportOptions::default(),
        )
        .unwrap();
        assert!(open(&path).is_ok());
    }

    #[test]
    fn a_failed_save_leaves_the_existing_file_alone() {
        let dir = TempDir::new("format-failed-save");
        let path = dir.file("photo.jpg");
        save(
            &sample(100),
            &path,
            FileFormat::Jpeg,
            &ExportOptions::default(),
        )
        .unwrap();
        let before = std::fs::read(&path).unwrap();

        // Too wide for a JPEG, so the export fails part-way.
        let too_big = Document::new(70_000, 1, BitDepth::U8).unwrap();
        assert!(save(&too_big, &path, FileFormat::Jpeg, &ExportOptions::default()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(dir.entries(), ["photo.jpg"]);
    }

    #[test]
    fn opening_a_missing_file_is_an_io_error() {
        let dir = TempDir::new("format-missing");
        assert!(matches!(
            open(&dir.file("nope.png")),
            Err(FormatError::Io(_))
        ));
    }
}
