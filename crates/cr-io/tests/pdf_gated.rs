//! PDF tests. Gated: they run only when a pdfium shared library is
//! available (CR_PDFIUM env var pointing at libpdfium.so, a library
//! in the working directory, or a system library) — the C# bundles
//! pdfium.dll, and we never commit binaries to the repo (test data
//! policy in docs/phase-1-kickoff.md).

use cr_io::pdf::is_available;
use cr_io::ComicProvider;

/// Builds a minimal 2-page PDF (portrait US Letter, 612x792 points)
/// with a proper xref table. Content-free pages; the provider layer
/// only counts and rasterizes them.
fn build_two_page_pdf() -> Vec<u8> {
    let header = b"%PDF-1.4\n";
    let objects = [
        b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n" as &[u8],
        b"2 0 obj\n<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>\nendobj\n",
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n",
        b"4 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n",
    ];
    let mut out = header.to_vec();
    let mut offsets = [0usize; 5];
    for (i, obj) in objects.iter().enumerate() {
        offsets[i + 1] = out.len();
        out.extend_from_slice(obj);
    }
    let xref_pos = out.len();
    out.extend_from_slice(b"xref\n0 5\n");
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets[1..] {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(b"trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n");
    out.extend_from_slice(format!("{xref_pos}\n%%EOF\n").as_bytes());
    out
}

#[test]
fn pdf_pages_render_and_format_check() {
    if !is_available() {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "comicrust-pdf-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("comic.pdf");
    std::fs::write(&path, build_two_page_pdf()).unwrap();

    let provider = ComicProvider::open(&path).unwrap();
    assert_eq!(provider.format().name, "PDF Document (PDF)");
    assert_eq!(provider.page_count(), 2);

    // Rendered page: JPEG bytes in native page order, hash = file hash.
    let page = provider.read_page(0).unwrap();
    assert_eq!(&page[..2], &[0xff, 0xd8], "JPEG SOI marker");
    assert_eq!(provider.create_hash().len(), 32);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn pdf_missing_library_degrades_to_empty() {
    if is_available() {
        return; // host has pdfium; the open test covers the real path
    }
    let dir = std::env::temp_dir().join("comicrust-pdf-missing");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("comic.pdf");
    std::fs::write(&path, build_two_page_pdf()).unwrap();
    let provider = ComicProvider::open(&path).unwrap();
    assert_eq!(provider.page_count(), 0);
    std::fs::remove_dir_all(&dir).ok();
}
