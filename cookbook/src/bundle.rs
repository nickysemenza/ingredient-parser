//! Portable saved-run exports. No extraction or model execution happens here.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::{Error, Extraction, Result};

mod html;

/// Version of the portable archive, independent of the extraction contract.
pub const BUNDLE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct BundleImage {
    /// Original ImageRef.path, preserved verbatim in extraction.json.
    pub source_path: String,
    /// Relative path inside this bundle; aliases share this path.
    pub path: String,
    pub mime: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct BundleManifest {
    pub format: String,
    pub version: u32,
    pub extraction: String,
    pub preview: String,
    pub source_sha256: String,
    pub run_id: String,
    pub incomplete: bool,
    pub images: Vec<BundleImage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct BundleExport {
    pub path: String,
    pub manifest: BundleManifest,
}

/// Export a saved run and its matching EPUB. Existing output is replaced only
/// after the whole ZIP has been written successfully. Inputs are never replaced.
/// The title/run-based default is beside the saved run, not in the current dir.
pub fn export(run: &Path, source: &Path, out: Option<&Path>) -> Result<BundleExport> {
    let extraction = crate::native::runs::load(run)?;
    let output = out.map(Path::to_path_buf).unwrap_or_else(|| {
        run.with_file_name(format!(
            "{}--{}.cookbook.zip",
            filename_part(&extraction.cookbook.source.title),
            filename_part(&extraction.report.run_id),
        ))
    });
    if let Ok(target) = output.canonicalize()
        && (target == run.canonicalize()? || target == source.canonicalize()?)
    {
        return Err(Error::Config(
            "Bundle output cannot replace the run or source EPUB".into(),
        ));
    }
    let mut reader = BufReader::new(File::open(source)?);
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    let sha = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if sha != extraction.cookbook.source.sha256 || sha != extraction.report.book.sha256 {
        return Err(Error::Config(
            "Source EPUB hash does not match the saved extraction".into(),
        ));
    }
    reader.seek(SeekFrom::Start(0))?;
    let mut archive = ZipArchive::new(reader)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    let manifest = write_bundle(&mut temporary, &extraction, &mut archive)?;
    temporary.as_file().sync_all()?;
    temporary.persist(&output).map_err(|e| Error::Io(e.error))?;
    Ok(BundleExport {
        path: output.to_string_lossy().into_owned(),
        manifest,
    })
}

fn filename_part(value: &str) -> String {
    let value: String = value
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let value = value
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    // Keep default names below common filesystem limits, including Unicode bytes.
    let mut end = value.len().min(80);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    if end == 0 {
        "book".into()
    } else {
        value[..end].to_owned()
    }
}

fn write_bundle<W: Write + Seek, R: Read + Seek>(
    output: W,
    extraction: &Extraction,
    archive: &mut ZipArchive<R>,
) -> Result<BundleManifest> {
    let book = &extraction.cookbook;
    let mut refs = BTreeMap::new();
    for image in book
        .cover
        .iter()
        .chain(book.items().flat_map(|item| item.photos()))
    {
        if !image.mime.starts_with("image/") {
            return Err(Error::Config(format!(
                "Invalid image MIME type for {}: {}",
                image.path, image.mime
            )));
        }
        if let Some(prior) = refs.insert(image.path.as_str(), image)
            && prior.mime != image.mime
        {
            return Err(Error::Config(format!(
                "Conflicting image MIME types for {}",
                image.path
            )));
        }
    }
    let mut zip = ZipWriter::new(output);
    let mut assets = BTreeMap::<String, (String, String)>::new();
    let mut manifest = BundleManifest {
        format: "cookbook-bundle".into(),
        version: BUNDLE_VERSION,
        extraction: "extraction.json".into(),
        preview: "index.html".into(),
        source_sha256: book.source.sha256.clone(),
        run_id: extraction.report.run_id.clone(),
        incomplete: extraction.report.incomplete || extraction.report.cancelled,
        images: Vec::new(),
    };
    for (source_path, image) in refs {
        if archive
            .file_names()
            .filter(|name| *name == source_path)
            .count()
            != 1
        {
            return Err(Error::Config(format!(
                "Source EPUB must contain exactly one referenced image: {source_path}"
            )));
        }
        let mut bytes = Vec::new();
        archive.by_name(source_path)?.read_to_end(&mut bytes)?;
        if bytes.is_empty() {
            return Err(Error::Config(format!(
                "Referenced image is empty: {source_path}"
            )));
        }
        let sha256 = crate::epub::open::sha256_hex(&bytes);
        let path = if let Some((path, mime)) = assets.get(&sha256) {
            if mime != &image.mime {
                return Err(Error::Config(format!(
                    "Identical image bytes have conflicting MIME types: {source_path}"
                )));
            }
            path.clone()
        } else {
            // MIME aliases are not ordered by preferred filename extension:
            // mime_guess lists jfif first for JPEG. Use the conventional suffix.
            let extension = match image.mime.as_str() {
                "image/jpeg" => "jpg",
                other => mime_guess::get_mime_extensions_str(other)
                    .and_then(|extensions| extensions.first())
                    .copied()
                    .unwrap_or("bin"),
            };
            let path = format!("images/{sha256}.{extension}");
            write_entry(&mut zip, &path, &bytes)?;
            assets.insert(sha256.clone(), (path.clone(), image.mime.clone()));
            path
        };
        manifest.images.push(BundleImage {
            source_path: source_path.into(),
            path,
            mime: image.mime.clone(),
            sha256,
            bytes: bytes.len() as u64,
        });
    }
    // A duplicate item id would make the review's links ambiguous.
    let mut ids = BTreeSet::new();
    if book.items().any(|item| !ids.insert(item.id())) {
        return Err(Error::Config(
            "Extraction contains duplicate item IDs".into(),
        ));
    }
    write_entry(
        &mut zip,
        &manifest.extraction,
        &serde_json::to_vec_pretty(extraction)?,
    )?;
    write_entry(
        &mut zip,
        &manifest.preview,
        html::render(extraction, &manifest).as_bytes(),
    )?;
    write_entry(
        &mut zip,
        "manifest.json",
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    zip.finish()?;
    Ok(manifest)
}

fn write_entry<W: Write + Seek>(zip: &mut ZipWriter<W>, path: &str, bytes: &[u8]) -> Result<()> {
    zip.start_file(
        path,
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
    )?;
    zip.write_all(bytes)?;
    Ok(())
}
