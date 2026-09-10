//! Port of the plugin's `scrapeengine.py` — the main processing loop.
//! The engine runs on a worker thread (the caller spawns it) and
//! drives the UI through the [`ScrapeUi`] callbacks; the books it
//! mutates are clones the UI commits on the main thread.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Datelike;

use cr_core::model::ComicBook;

use crate::bookdata::BookData;
use crate::config::Configuration;
use crate::cv::connection::CvError;
use crate::cv::models::{Issue, IssueRef, SeriesRef};
use crate::cv::queries::Cv;
use crate::matching::matchscore::MatchScore;
use crate::utils::natural_key;

/// The state of one book in the main loop (the C# `BookStatus`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookStatus {
    Scraped,
    Skipped,
    Unscraped,
    Delayed,
}

/// The series dialog result (the C# `SeriesFormResult`).
#[derive(Clone, Debug)]
pub enum SeriesResult {
    /// The user chose a series ("OK").
    Ok(SeriesRef),
    /// The user clicked "Show Issues" with a chosen series.
    Show(SeriesRef),
    /// Search again with new terms.
    Search,
    Skip,
    Permskip,
    Cancel,
}

/// The issue dialog result (the C# `IssueFormResult`).
#[derive(Clone, Debug)]
pub enum IssueResult {
    Ok(IssueRef),
    Back,
    Skip,
    Permskip,
    Cancel,
}

/// What a progress callback reports (the C# progbar texts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressKind {
    /// The series search: the number of matches so far.
    SeriesSearch,
    /// The issue-list load: the 0..1 completion ratio.
    IssueList,
}

/// The UI half of the protocol. Implemented by the cr-ui wizard (over
/// channels) and by the scripted fake in the tests. All methods block
/// until the UI answers (or return immediately for the notify
/// events).
pub trait ScrapeUi: Send {
    /// Ask for series search terms (the C# `SearchForm`); `None`
    /// cancels the whole scrape.
    fn request_search_terms(&mut self, book_caption: &str, failed_terms: &str) -> Option<String>;
    /// The series selection dialog (the C# `SeriesForm`).
    fn request_series(
        &mut self,
        book_caption: &str,
        terms: &str,
        refs: &[SeriesRef],
    ) -> SeriesResult;
    /// The issue selection dialog (the C# `IssueForm`); `hint` is the
    /// auto-identified issue the user can confirm.
    fn request_issue(
        &mut self,
        book_caption: &str,
        series: &SeriesRef,
        issues: &[IssueRef],
        hint: Option<&IssueRef>,
        force: bool,
    ) -> IssueResult;
    /// The "no issues in this series" warning (the C# MessageBox).
    fn no_issues_available(&mut self, series_name: &str);
    /// A book started scraping (the C# `start_scrape_listeners`).
    fn book_started(&mut self, caption: &str, remaining: usize);
    /// A book reached a final state (the comicform status list).
    fn book_finished(&mut self, caption: &str, status: BookStatus);
    /// A book was scraped: the finished clone lands here (the UI
    /// commits it through the library pipeline).
    fn book_scraped(&mut self, book: &ComicBook);
    /// Search progress (matches so far, expected calls) and issue-
    /// list progress (0..1).
    fn progress(&mut self, kind: ProgressKind, value: f64);
    /// A scrape-path error the user should see (a query failure, an
    /// API status). The run continues — the book resolves later.
    fn error(&mut self, message: &str);
}

/// The scrape result counts (the C# `__status`: [scraped, skipped]).
pub type ScrapeSummary = (usize, usize);

/// The scrape engine. `stop` is shared with the UI: setting it
/// cancels the whole scrape at the next check (the C# `cancel`).
/// The custom-thumbnail installer the UI injects: stores the
/// downloaded cover bytes and returns the key text (the C#
/// `ImagePool.AddCustomThumbnail`; the engine stays image-pool-free,
/// ADR-031).
pub type ThumbInstaller = Arc<dyn Fn(&[u8]) -> Option<String> + Send + Sync>;

pub struct ScrapeEngine {
    pub config: Configuration,
    pub stop: Arc<AtomicBool>,
    /// The user's prior series keys (the C# series.dat).
    pub prior_series: std::collections::HashSet<String>,
    /// The test seam: overrides the configured scrape delay.
    pub scrape_delay_override: Option<Duration>,
    /// The series keys the user chose this run (the C#
    /// `MatchScore.record_choice` collection; the wizard persists
    /// them into prior_series.json at Done).
    chosen: std::sync::Mutex<Vec<String>>,
    /// The injected custom-thumbnail installer (the C#
    /// `App.SetCustomBookThumbnail` path).
    pub thumb_installer: Option<ThumbInstaller>,
}

/// One cache entry per resolved series (the C# `ScrapedSeries`).
struct ScrapedSeries {
    series_ref: Option<SeriesRef>,
    issue_refs: Vec<IssueRef>,
}

impl ScrapedSeries {
    fn new(series_ref: Option<SeriesRef>) -> Self {
        ScrapedSeries {
            series_ref,
            issue_refs: Vec::new(),
        }
    }
}

/// The scrape book-keeping for one run (the C# `scrape_cache` +
/// `__status`).
struct Run {
    cache: HashMap<String, ScrapedSeries>,
    scraped: usize,
    remaining: usize,
}

impl ScrapeEngine {
    pub fn new(
        config: Configuration,
        stop: Arc<AtomicBool>,
        prior_series: std::collections::HashSet<String>,
    ) -> Self {
        ScrapeEngine {
            config,
            stop,
            prior_series,
            scrape_delay_override: None,
            chosen: std::sync::Mutex::new(Vec::new()),
            thumb_installer: None,
        }
    }

    /// The series keys the user chose this run (for
    /// `prior_series.json` persistence).
    pub fn chosen(&self) -> Vec<String> {
        self.chosen.lock().unwrap().clone()
    }

    fn cancelled(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// The main processing loop. Returns (scraped, skipped).
    pub fn scrape(
        &self,
        books: Vec<ComicBook>,
        ui: &mut dyn ScrapeUi,
        cv: &mut Cv,
    ) -> (usize, usize) {
        let mut run = Run {
            cache: HashMap::new(),
            scraped: 0,
            remaining: books.len(),
        };
        crate::log::debug(&format!(
            "scrape starts: {} book(s), fast-rescrape {}, autochoose {}",
            books.len(),
            self.config.fast_rescrape,
            self.config.autochoose_series
        ));

        // 1. wrap the books and sort: the fast rescrapes first, then
        //    by series + padded issue number (the C# __sort_books)
        let books: Vec<Book> = books
            .into_iter()
            .map(|book| {
                let data = BookData::from_book(&book, &self.config);
                Book { comic: book, data }
            })
            .collect();
        let mut books = sort_books(books, &self.config);

        // 2. the main processing loop
        let mut i = 0usize;
        let orig_length = books.len();
        while i < books.len() {
            if self.cancelled() {
                break;
            }
            let delayed = i >= orig_length;
            // 2a. the scrape delay between books (not for delayed
            //     books or the first one)
            if i != 0 && !delayed {
                self.wait_until_ready();
                if self.cancelled() {
                    break;
                }
            }

            let caption = books[i].data.caption();
            crate::log::debug(&format!(
                "scraping next book: {caption} (delayed: {delayed})"
            ));
            ui.book_started(&caption, books.len() - i);

            // 2b. keep scraping that book until scraped, skipped, or
            //     delayed to the end
            let mut manual_search = false;
            let fast_rescrape = self.config.fast_rescrape && !delayed;
            let autoscrape =
                self.config.autochoose_series && !self.config.confirm_issue && !delayed;
            loop {
                if self.cancelled() {
                    break;
                }
                let status = self.scrape_book(
                    &mut books[i],
                    &mut run,
                    manual_search,
                    fast_rescrape,
                    autoscrape,
                    ui,
                    cv,
                );
                match status {
                    BookStatus::Unscraped => manual_search = true, // retry with manual terms
                    BookStatus::Scraped => {
                        run.scraped += 1;
                        run.remaining -= 1;
                        ui.book_finished(&caption, BookStatus::Scraped);
                        break;
                    }
                    BookStatus::Skipped => {
                        ui.book_finished(&caption, BookStatus::Skipped);
                        break;
                    }
                    BookStatus::Delayed => {
                        if !delayed {
                            let again = books[i].clone();
                            books.push(again);
                        }
                        break;
                    }
                }
            }
            i += 1;
        }
        (run.scraped, run.remaining)
    }
}

#[derive(Clone)]
struct Book {
    comic: ComicBook,
    data: BookData,
}

impl Book {
    fn issue_ref(&self) -> Option<IssueRef> {
        self.data.extract_issue_ref()
    }
}

impl ScrapeEngine {
    /// The heart of the loop (the C# `__scrape_book`): identifies the
    /// issue for one book, then copies the details in.
    #[allow(clippy::too_many_arguments)]
    fn scrape_book(
        &self,
        book: &mut Book,
        run: &mut Run,
        manual_search: bool,
        fast_rescrape: bool,
        autoscrape: bool,
        ui: &mut dyn ScrapeUi,
        cv: &mut Cv,
    ) -> BookStatus {
        let caption = book.data.caption();

        // 1. the skip tag: silently skip
        if book.data.skip_tagged() {
            crate::log::debug("found the CVDBSKIP tag: skipping the book");
            return BookStatus::Skipped;
        }

        // 2. the fast rescrape: the book knows its issue from a
        //    previous scrape
        if fast_rescrape {
            if let Some(issue_ref) = book.issue_ref() {
                crate::log::debug(&format!(
                    "rescraping: the book identifies its issue as {issue_ref}"
                ));
                let slow = self.config.advanced().update_rating;
                match cv.query_issue(&issue_ref, slow) {
                    Ok(issue) => {
                        self.apply_issue(book, &issue, cv, ui);
                        return BookStatus::Scraped;
                    }
                    Err(err) => {
                        crate::log::debug(&format!("fast rescrape of {issue_ref} failed: {err}"));
                        ui.error(&format!(
                            "Rescrape query failed: {err} (the book retries at the end)"
                        ));
                        return BookStatus::Delayed; // retry manually later
                    }
                }
            }
        }

        // 3. the unique series key drives the series cache
        let key = book
            .data
            .unique_series(self.config.advanced().ignore_folders);

        // 3a. the book may know its series from a previous scrape
        if !run.cache.contains_key(&key) && !manual_search {
            if let Some(series_ref) = book.data.extract_series_ref() {
                run.cache
                    .insert(key.clone(), ScrapedSeries::new(Some(series_ref)));
            }
        }

        // 3b. the magic cvinfo file in the book's folder
        if !run.cache.contains_key(&key) && !manual_search {
            if let Some(series_ref) = cv.check_magic_file(&book.data.path) {
                run.cache
                    .insert(key.clone(), ScrapedSeries::new(Some(series_ref)));
            }
        }

        // 3c. the auto-scrape algorithm (cover matching)
        if !run.cache.contains_key(&key) && autoscrape {
            crate::log::debug("trying to match the book automatically\u{2026}");
            match self.auto_match(book, cv) {
                Ok(Some(series_ref)) => {
                    run.cache
                        .insert(key.clone(), ScrapedSeries::new(Some(series_ref)));
                }
                _ => return BookStatus::Delayed,
            }
        }

        // 3d. the series search (auto or manual terms)
        let mut series_refs: Option<Vec<SeriesRef>> = None;
        let mut terms = book.data.series.clone();
        if !run.cache.contains_key(&key) {
            if manual_search || terms.is_empty() {
                match ui.request_search_terms(&caption, "") {
                    Some(t) if !t.trim().is_empty() => terms = t,
                    _ => return BookStatus::Skipped, // cancel the whole scrape
                }
            }
            let refs = match cv.query_series_refs(
                &terms,
                &self
                    .config
                    .advanced()
                    .ignored_searchterms
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>(),
                self.config.advanced().max_search_results,
                &mut |matches, expected| {
                    ui.progress(ProgressKind::SeriesSearch, matches as f64);
                    let _ = expected;
                    self.cancelled()
                },
            ) {
                Ok(refs) => refs,
                Err(err) => {
                    crate::log::debug(&format!(
                        "series search for \u{201C}{terms}\u{201D} failed: {err}"
                    ));
                    ui.error(&format!("Series search failed: {err}"));
                    return BookStatus::Unscraped;
                }
            };
            crate::log::debug(&format!(
                "search for \u{201C}{terms}\u{201D}: {} series found",
                refs.len()
            ));
            if refs.is_empty() {
                return BookStatus::Unscraped; // the search dialog retries manually
            }
            series_refs = Some(refs);
        }

        // 4-5. resolve the series (when needed), then the issue
        loop {
            let mut force_issue_dialog = self.config.confirm_issue;
            if !run.cache.contains_key(&key) {
                let Some(refs) = series_refs.take() else {
                    return BookStatus::Unscraped;
                };
                if refs.is_empty() {
                    return BookStatus::Unscraped;
                }
                match ui.request_series(&caption, &terms, &refs) {
                    SeriesResult::Cancel => return BookStatus::Skipped,
                    SeriesResult::Skip => return BookStatus::Skipped,
                    SeriesResult::Permskip => {
                        skip_forever(book, &self.config);
                        return BookStatus::Skipped;
                    }
                    SeriesResult::Search => return BookStatus::Unscraped,
                    SeriesResult::Ok(series_ref) => {
                        run.cache
                            .insert(key.clone(), ScrapedSeries::new(Some(series_ref)));
                    }
                    SeriesResult::Show(series_ref) => {
                        force_issue_dialog = true;
                        run.cache
                            .insert(key.clone(), ScrapedSeries::new(Some(series_ref)));
                    }
                }
            }

            let Some(entry) = run.cache.get(&key) else {
                return BookStatus::Unscraped;
            };
            let Some(series_ref) = entry.series_ref.clone() else {
                return BookStatus::Unscraped;
            };

            // 5. the issue resolution
            let mut issue_refs = entry.issue_refs.clone();
            let issue_choice = self.choose_issue_ref(
                book,
                &series_ref,
                &mut issue_refs,
                force_issue_dialog,
                ui,
                cv,
            );
            if let Some(entry) = run.cache.get_mut(&key) {
                entry.issue_refs = issue_refs;
            }

            match issue_choice {
                IssueResult::Cancel => return BookStatus::Skipped,
                IssueResult::Skip => return BookStatus::Skipped,
                IssueResult::Permskip => {
                    if force_issue_dialog && !self.config.confirm_issue {
                        run.cache.remove(&key); // the series choice was wrong
                    }
                    skip_forever(book, &self.config);
                    return BookStatus::Skipped;
                }
                IssueResult::Back => {
                    run.cache.remove(&key); // back to the series dialog
                }
                IssueResult::Ok(issue_ref) => {
                    crate::log::debug(&format!("issue resolved: {issue_ref}"));
                    self.chosen
                        .lock()
                        .unwrap()
                        .push(series_ref.series_key.to_string());
                    let slow = self.config.advanced().update_rating;
                    match cv.query_issue(&issue_ref, slow) {
                        Ok(issue) => {
                            self.apply_issue(book, &issue, cv, ui);
                            return BookStatus::Scraped;
                        }
                        Err(err) => {
                            crate::log::debug(&format!(
                                "issue details for {issue_ref} failed: {err}"
                            ));
                            ui.error(&format!(
                                "Issue query failed: {err} (the book retries at the end)"
                            ));
                            return BookStatus::Delayed;
                        }
                    }
                }
            }
        }
    }

    /// `auto_match`: the automatcher with the book's decoded cover.
    fn auto_match(&self, book: &Book, cv: &mut Cv) -> Result<Option<SeriesRef>, CvError> {
        let page0 = self.book_cover(book);
        let score = MatchScore::new(self.prior_series.clone());
        crate::matching::automatcher::find_series_ref(
            &book.data,
            &self.config,
            cv,
            &score,
            current_year(),
            page0.as_ref(),
            &mut |_matches, _expected| self.cancelled(),
        )
    }

    /// The book's front cover, decoded for the automatcher (the C#
    /// App.GetComicPage(0); an archive read on this worker thread).
    fn book_cover(&self, book: &Book) -> Option<cr_image::Image> {
        if book.data.path.is_empty() {
            return None;
        }
        let path = std::path::PathBuf::from(&book.data.path);
        let provider = cr_io::ComicProvider::open(&path).ok()?;
        let bytes = provider.read_page(0)?;
        cr_image::decode(&bytes).ok()
    }

    /// The copy step (the C# `book.update(issue)`): the massage rules
    /// run, the fields land in the comic clone, and the UI gets the
    /// finished book.
    fn apply_issue(&self, book: &mut Book, issue: &Issue, cv: &mut Cv, ui: &mut dyn ScrapeUi) {
        let now = now_text();
        book.data.update(
            issue,
            &self.config,
            &now,
            None, // the session alt-cover choice rides T7
        );
        book.data.apply_to(&mut book.comic);
        self.install_thumbnail(book, issue, cv);
        ui.book_scraped(&book.comic);
    }

    /// The fileless-book thumbnail install (the C#
    /// `PluginBookData.update` cover-url branch): only fileless books,
    /// only when the prefs ask for it, never over a preserved thumb.
    fn install_thumbnail(&self, book: &mut Book, issue: &Issue, cv: &mut Cv) {
        let config = &self.config;
        // Only FILELESS books install a custom thumbnail (an empty
        // path); the C# never touches a linked book's cover.
        if !config.download_thumbs || !book.data.path.is_empty() {
            return;
        }
        if book.comic.custom_thumbnail_key.is_some() && config.preserve_thumbs {
            return;
        }
        let url = if !book.data.cover_url.is_empty() {
            Some(book.data.cover_url.clone())
        } else {
            issue.image_urls.first().cloned()
        };
        let Some(url) = url else {
            return;
        };
        let Some(installer) = &self.thumb_installer else {
            return;
        };
        let Some(bytes) = cv.query_image(&url) else {
            return;
        };
        if let Some(key) = installer(&bytes) {
            book.comic.custom_thumbnail_key = Some(key);
        }
    }

    /// `__wait_until_ready`: the per-book scrape delay, cancellable.
    fn wait_until_ready(&self) {
        let secs = match self.scrape_delay_override {
            Some(d) => d.as_secs(),
            None => self.config.advanced().scrape_delay.max(0) as u64,
        };
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline && !self.cancelled() {
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// `__choose_issue_ref`: resolves the issue automatically when
    /// possible; the dialog shows only when the number is ambiguous
    /// or the user forced it.
    #[allow(clippy::too_many_arguments)]
    fn choose_issue_ref(
        &self,
        book: &Book,
        series_ref: &SeriesRef,
        issue_refs: &mut Vec<IssueRef>,
        force: bool,
        ui: &mut dyn ScrapeUi,
        cv: &mut Cv,
    ) -> IssueResult {
        let caption = book.data.caption();
        let issue_num = book.data.issue_num.trim().to_string();

        // 1. the shortcut: resolve the issue number directly
        if issue_refs.is_empty() && !issue_num.is_empty() && !force {
            if let Ok(Some(issue_ref)) = cv.query_issue_ref(series_ref, &issue_num) {
                return IssueResult::Ok(issue_ref);
            }
        }

        // 2. the full issue list fills the caller's cache (and the
        //    number matching below)
        if issue_refs.is_empty() {
            match cv.query_issue_refs(series_ref, &mut |ratio| {
                ui.progress(ProgressKind::IssueList, ratio);
                self.cancelled()
            }) {
                Ok(loaded) => {
                    if self.cancelled() {
                        return IssueResult::Cancel;
                    }
                    if loaded.is_empty() {
                        ui.no_issues_available(series_ref.series_name());
                        return IssueResult::Back;
                    }
                    *issue_refs = loaded;
                }
                Err(_) => return IssueResult::Cancel,
            }
        }

        // 3. the issue number may be in the list: an ambiguous number
        //    (the same number twice) forces the dialog
        if !issue_num.is_empty() {
            let matches: Vec<&IssueRef> = issue_refs
                .iter()
                .filter(|r| natural_key(&r.issue_num) == natural_key(&issue_num))
                .collect();
            if matches.len() == 1 {
                return IssueResult::Ok(matches[0].clone());
            }
            if matches.len() > 1 {
                // the same issue number appears more than once: pick
            }
        }

        // 4. no number, one issue: the only choice
        if issue_num.is_empty() && issue_refs.len() == 1 {
            return IssueResult::Ok(issue_refs[0].clone());
        }

        // 5. the dialog decides
        ui.request_issue(&caption, series_ref, issue_refs, None, force)
    }
}

fn current_year() -> i32 {
    chrono::Local::now().year()
}

fn now_text() -> String {
    chrono::Local::now().format("%Y.%m.%d %H:%M:%S").to_string()
}

/// `ComicBook.skip_forever`: the CVDBSKIP flag lands in the clone.
fn skip_forever(book: &mut Book, config: &Configuration) {
    book.data.skip_forever(config, &now_text());
    book.data.apply_to(&mut book.comic);
}

/// `__sort_books`: the fast rescrapes first (saves the user
/// interaction until the end), then series name + padded issue
/// number.
fn sort_books(books: Vec<Book>, config: &Configuration) -> Vec<Book> {
    let mut fast: Vec<Book> = Vec::new();
    let mut slow: Vec<Book> = Vec::new();
    if config.fast_rescrape {
        for book in books {
            if book.data.skip_tagged() || book.data.extract_issue_ref().is_some() {
                fast.push(book);
            } else {
                slow.push(book);
            }
        }
    } else {
        slow = books;
    }

    let ignore = config.advanced().ignore_folders;
    let cmp = |a: &Book, b: &Book| -> std::cmp::Ordering {
        let ka = a.data.unique_series(ignore);
        let kb = b.data.unique_series(ignore);
        let mut ord = ka.cmp(&kb);
        if ord == std::cmp::Ordering::Equal {
            ord = pad_issue_number(&a.data.issue_num).cmp(&pad_issue_number(&b.data.issue_num));
        }
        ord
    };
    slow.sort_by(&cmp);
    fast.sort_by(&cmp);
    fast.extend(slow);
    fast
}

/// The C# `pad`: zero-pads the number by its float value so
/// "2.5" sorts before "10" and after "2".
fn pad_issue_number(num: &str) -> String {
    let lowered = num.to_lowercase();
    let stripped = lowered.trim_matches(|c| "abcdefgh".contains(c));
    let Ok(f) = stripped.parse::<f64>() else {
        return num.to_string();
    };
    if f < 10.0 {
        format!("000{num}")
    } else if f < 100.0 {
        format!("00{num}")
    } else if f < 1000.0 {
        format!("0{num}")
    } else {
        num.to_string()
    }
}
