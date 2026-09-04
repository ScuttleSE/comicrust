//! The page-entry merge — the C# open semantics.
//!
//! `ComicBookNavigator.ProviderIndexRetrievalCompleted` sets
//! `PageCount = ProviderPageCount` and `TrimExcessPageInfo`s the
//! stored list: the DISPLAY sequence is always the provider count,
//! and each page's info is the stored entry when one exists at that
//! position (a partial metadata list overlays the full provider
//! sequence — `GetPage(i)` returns the entry or a default).

use cr_core::model::comic_book::ComicBook;
use cr_core::model::comic_page_info::ComicPageInfo;
use cr_io::ComicProvider;

/// The merged display entries: `provider.page_count()` rows; row `i`
/// is the stored entry when present, else a fresh entry carrying the
/// provider key and its archive index.
pub fn merged_page_entries(book: &ComicBook, provider: &ComicProvider) -> Vec<ComicPageInfo> {
    let count = provider.page_count();
    let stored = &book.info.pages;
    (0..count)
        .map(|i| match stored.get(i) {
            Some(entry) => {
                let mut e = entry.clone();
                // The read index must resolve for reordered books: a
                // stored entry without a usable Image falls back to
                // its position (the C# fabricated defaults carry
                // Image = position too).
                if e.image_index() < 0 {
                    e.set_image_index(i as i32);
                }
                e
            }
            None => {
                let mut e = ComicPageInfo {
                    key: provider.pages().get(i).map(|p| p.name.clone()),
                    ..Default::default()
                };
                e.set_image_index(i as i32);
                e
            }
        })
        .collect()
}

/// Convenience: `None` when the provider could not be opened (the
/// caller keeps the stored list).
pub fn merged_pages_or_none(
    book: &ComicBook,
    path: &std::path::Path,
) -> Option<Vec<ComicPageInfo>> {
    let provider = ComicProvider::open(path).ok()?;
    Some(merged_page_entries(book, &provider))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::model::enums::ComicPageType;
    use cr_core::xml::scalar::CrGuid;

    /// A folder comic needs no archive tools: three text files as
    /// "pages" (the folder provider lists any file).
    fn folder_provider(name: &str, files: &[&str]) -> (tempdir::TempDirPath, ComicProvider) {
        let dir = tempdir::TempDirPath::new(name);
        for f in files {
            std::fs::write(dir.path().join(f), "x").unwrap();
        }
        let provider = ComicProvider::open(dir.path()).expect("folder provider");
        (dir, provider)
    }

    #[test]
    fn partial_stored_list_overlays_the_provider_count() {
        let (_dir, provider) = folder_provider("cr-ui-merge-a", &["a.jpg", "b.jpg", "c.jpg"]);
        let mut book = ComicBook {
            id: CrGuid::EMPTY,
            ..Default::default()
        };
        // The migrated shape: one stored FrontCover entry.
        let mut cover = ComicPageInfo::default();
        cover.set_image_index(0);
        cover.page_type = ComicPageType(1);
        book.info.pages = vec![cover];

        let merged = merged_page_entries(&book, &provider);
        assert_eq!(merged.len(), 3, "the display is the provider count");
        assert_eq!(merged[0].page_type, ComicPageType(1), "the stored overlay");
        assert_eq!(merged[1].image_index(), 1);
        assert_eq!(merged[2].image_index(), 2);
        assert!(merged[1].key.is_some(), "the provider key fills");
    }

    #[test]
    fn entries_without_a_usable_image_fall_back_to_their_position() {
        let (_dir, provider) = folder_provider("cr-ui-merge-b", &["a.jpg", "b.jpg"]);
        let mut book = ComicBook::default();
        let mut e = ComicPageInfo::default(); // Image default → index -1
        e.set_image_index(-1);
        book.info.pages = vec![e.clone(), e];

        let merged = merged_page_entries(&book, &provider);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].image_index(), 0);
        assert_eq!(merged[1].image_index(), 1);
    }
}

/// A temp dir handle that removes itself on drop (the test seams use
/// std::temp semantics; cr-ui has no tempfile dependency).
#[cfg(test)]
pub(crate) mod tempdir {
    use std::path::PathBuf;

    pub struct TempDirPath(PathBuf);

    impl TempDirPath {
        pub fn new(name: &str) -> TempDirPath {
            let dir = std::env::temp_dir().join(format!(
                "{name}-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            TempDirPath(dir)
        }

        pub fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDirPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
