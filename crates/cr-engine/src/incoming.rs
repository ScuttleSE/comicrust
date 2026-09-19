//! Configuration and path-role checks for incoming folders.

/// The `[plugins.incoming]` table in the unified configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IncomingConfig {
    #[serde(default)]
    pub incoming_folders: Vec<String>,
    #[serde(default)]
    pub last_organizer_profile: String,
    #[serde(default)]
    pub find_in_incoming_profile: String,
}

impl IncomingConfig {
    /// Returns true when `path` is below a configured incoming folder.
    pub fn is_incoming_path(&self, path: &str) -> bool {
        self.incoming_folders
            .iter()
            .any(|folder| crate::duplicates::under_path(path, folder))
    }

    /// Incoming folders are always monitored folders.
    pub fn is_monitored_path(&self, path: &str) -> bool {
        self.is_incoming_path(path)
    }
}

/// One configured folder and its staged role.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderRole {
    pub folder: String,
    pub incoming: bool,
    pub watch: bool,
}

/// Rejects duplicate, ancestor, and descendant configured roots.
pub fn validate_folder_roots(roles: &[FolderRole]) -> Result<(), String> {
    for (index, left) in roles.iter().enumerate() {
        for right in &roles[index + 1..] {
            if same_or_under(&left.folder, &right.folder)
                || same_or_under(&right.folder, &left.folder)
            {
                return Err(format!(
                    "Folder roots must not overlap:\n{}\n{}",
                    left.folder, right.folder
                ));
            }
        }
    }
    Ok(())
}

/// Returns the library records that newly assigned Incoming roots must receive.
pub fn conversion_indexes(
    books: &[ComicBook],
    old_config: &IncomingConfig,
    roles: &[FolderRole],
) -> Vec<usize> {
    let added: Vec<&str> = roles
        .iter()
        .filter(|role| role.incoming && !root_is_configured(&role.folder, old_config))
        .map(|role| role.folder.as_str())
        .collect();
    books
        .iter()
        .enumerate()
        .filter(|(_, book)| added.iter().any(|root| under_root(&book.file_path, root)))
        .map(|(index, _)| index)
        .collect()
}

/// Rejects removal of a configured row or Incoming role that still owns records.
pub fn validate_unresolved_removals(
    old_roles: &[FolderRole],
    staged_roles: &[FolderRole],
    incoming_books: &[ComicBook],
) -> Result<(), String> {
    for old in old_roles {
        let staged = staged_roles
            .iter()
            .find(|role| same_root(&role.folder, &old.folder));
        let removes_folder = staged.is_none();
        let removes_incoming = old.incoming && staged.is_some_and(|role| !role.incoming);
        if (removes_folder || removes_incoming)
            && incoming_books
                .iter()
                .any(|book| under_root(&book.file_path, &old.folder))
        {
            let count = incoming_books
                .iter()
                .filter(|book| under_root(&book.file_path, &old.folder))
                .count();
            return Err(format!(
                "Cannot remove '{}' or its Incoming role. Resolve the {count} incoming record(s) under this folder first.",
                old.folder
            ));
        }
    }
    Ok(())
}

fn root_is_configured(root: &str, config: &IncomingConfig) -> bool {
    config
        .incoming_folders
        .iter()
        .any(|configured| same_root(root, configured))
}

fn path_components(value: &str) -> Vec<String> {
    value
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn same_root(left: &str, right: &str) -> bool {
    let left = path_components(left);
    !left.is_empty() && left == path_components(right)
}

/// Returns true when two configured roots have the same path components.
pub fn roots_equal(left: &str, right: &str) -> bool {
    same_root(left, right)
}

fn same_or_under(path: &str, root: &str) -> bool {
    let path = path_components(path);
    let root = path_components(root);
    !root.is_empty() && path.len() >= root.len() && path.starts_with(&root)
}

fn under_root(path: &str, root: &str) -> bool {
    let path = path_components(path);
    let root = path_components(root);
    !root.is_empty() && path.len() > root.len() && path.starts_with(&root)
}

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use cr_core::database::list_items::{ComicListItem, SmartListItem};
use cr_core::model::comic_book::ComicBook;
use cr_core::paths::{incoming_file, incoming_lists_file, Paths};
use cr_core::xml::scalar::CrGuid;
use cr_core::xml::{Emitter, Tok, XmlError, XmlReader};

use crate::matcher::{book_view, eval::grouped_duplicate_indexes};

/// A catalog stored independently from `ComicDb.xml`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IncomingCatalog {
    pub books: Vec<ComicBook>,
}

/// The normalized shadow identity used by incoming views.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IncomingIdentity {
    pub series: String,
    pub volume: i32,
    pub format: String,
    pub language: String,
}

/// A comic issue number parsed with the existing ComicRack rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IssueNumber(f32);

impl IssueNumber {
    pub fn parse(text: &str) -> Option<Self> {
        let (valid, number) = crate::matcher::text_number::parse_comic_number(text);
        (valid && number.is_finite()).then_some(Self(number))
    }

    pub fn value(self) -> f32 {
        self.0
    }
}

/// Cached gaps supplied by the caller, keyed by incoming identity.
pub type ExternalGapCache = HashMap<IncomingIdentity, Vec<IssueNumber>>;

/// Dynamic-view membership for one incoming record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingClassification {
    /// Index into the input incoming-book slice. Every input has one result.
    pub index: usize,
    pub duplicate: bool,
    pub library_duplicate: bool,
    pub incoming_duplicate: bool,
    pub gap_fill: bool,
    pub new_series: bool,
    pub needs_review: bool,
}

/// The Incoming candidates for one selected synthetic Missing Issues row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingIncomingMatch {
    /// Index into the selected missing-row slice.
    pub missing_index: usize,
    /// Indexes into the Incoming catalog slice.
    pub incoming_indexes: Vec<usize>,
}

/// Matches Missing Issues rows to Incoming books by series, volume, and number.
///
/// Format and language do not participate because a Missing Issues row does not
/// carry those values. Series and number use the same normalization as the
/// Incoming Gap Fills view.
pub fn find_missing_incoming_matches(
    missing: &[ComicBook],
    incoming: &[ComicBook],
) -> Vec<MissingIncomingMatch> {
    type Key = (String, i32, u32);

    let mut by_issue: HashMap<Key, Vec<usize>> = HashMap::new();
    for (index, book) in incoming.iter().enumerate() {
        let Some(identity) = incoming_identity(book) else {
            continue;
        };
        let Some(number) = issue_number(book) else {
            continue;
        };
        by_issue
            .entry((identity.series, identity.volume, number.value().to_bits()))
            .or_default()
            .push(index);
    }

    missing
        .iter()
        .enumerate()
        .map(|(missing_index, book)| {
            let incoming_indexes = IssueNumber::parse(&book.info.number)
                .and_then(|number| {
                    by_issue.get(&(
                        normalize_series(&book.info.series),
                        book.info.volume,
                        number.value().to_bits(),
                    ))
                })
                .cloned()
                .unwrap_or_default();
            MissingIncomingMatch {
                missing_index,
                incoming_indexes,
            }
        })
        .collect()
}

#[derive(Debug)]
pub enum IncomingError {
    Io(std::io::Error),
    Xml(XmlError),
}

impl std::fmt::Display for IncomingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "incoming catalog I/O error: {error}"),
            Self::Xml(error) => write!(f, "incoming catalog XML error: {error}"),
        }
    }
}

impl std::error::Error for IncomingError {}

impl From<std::io::Error> for IncomingError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<XmlError> for IncomingError {
    fn from(error: XmlError) -> Self {
        Self::Xml(error)
    }
}

impl IncomingCatalog {
    pub fn load(paths: &Paths) -> Result<Self, IncomingError> {
        Self::load_from(&incoming_file(paths))
    }

    pub fn load_from(path: &Path) -> Result<Self, IncomingError> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default())
            }
            Err(error) => return Err(error.into()),
        };
        let mut input = BufReader::new(file);
        let mut reader = XmlReader::new(&mut input);
        match reader.next_tok()? {
            Tok::Start(start) if start.name == "IncomingDatabase" => {}
            Tok::Start(start) => {
                return Err(XmlError(format!("unexpected root <{}>", start.name)).into())
            }
            Tok::Eof => return Err(XmlError("empty incoming catalog".into()).into()),
            token => {
                return Err(
                    XmlError(format!("unexpected token before incoming root: {token:?}")).into(),
                )
            }
        }
        let mut books = Vec::new();
        loop {
            match reader.next_tok()? {
                Tok::Start(start) if start.name == "Book" => {
                    books.push(ComicBook::read_xml(&start, &mut reader)?);
                }
                Tok::Start(start) => reader.skip_element(&start.name)?,
                Tok::End(name) if name == "IncomingDatabase" => break,
                Tok::Eof => return Err(XmlError("eof before </IncomingDatabase>".into()).into()),
                _ => {}
            }
        }
        Ok(Self { books })
    }

    pub fn save(&self, paths: &Paths) -> Result<(), IncomingError> {
        self.save_to(&incoming_file(paths))
    }

    pub fn save_to(&self, path: &Path) -> Result<(), IncomingError> {
        let bytes = self.to_bytes()?;
        cr_core::durable::durable_replace(path, &bytes).map_err(IncomingError::Io)
    }

    /// Serializes the complete catalog for a durable transaction after-image.
    pub fn to_bytes(&self) -> Result<Vec<u8>, std::io::Error> {
        let mut output = Vec::new();
        let mut emitter = Emitter::new(&mut output)?;
        emitter.root("IncomingDatabase")?;
        for book in &self.books {
            book.write_xml(&mut emitter)?;
        }
        emitter.end()?;
        emitter.finish()?;
        Ok(output)
    }
}

/// User smart lists stored independently from both comic catalogs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IncomingLists {
    pub lists: Vec<SmartListItem>,
}

#[derive(Debug)]
pub enum IncomingListError {
    Io(std::io::Error),
    Xml(XmlError),
    EmptyId,
    DuplicateId(CrGuid),
    MissingList(CrGuid),
    Evaluation(crate::lists::ListEvaluationError),
}

impl std::fmt::Display for IncomingListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "incoming list I/O error: {error}"),
            Self::Xml(error) => write!(f, "incoming list XML error: {error}"),
            Self::EmptyId => write!(f, "incoming smart-list ID must not be empty"),
            Self::DuplicateId(id) => write!(f, "duplicate incoming smart-list ID: {id}"),
            Self::MissingList(id) => write!(f, "incoming smart list does not exist: {id}"),
            Self::Evaluation(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for IncomingListError {}

impl From<std::io::Error> for IncomingListError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<XmlError> for IncomingListError {
    fn from(error: XmlError) -> Self {
        Self::Xml(error)
    }
}

impl IncomingLists {
    pub fn load(paths: &Paths) -> Result<Self, IncomingListError> {
        Self::load_from(&incoming_lists_file(paths))
    }

    pub fn load_from(path: &Path) -> Result<Self, IncomingListError> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default())
            }
            Err(error) => return Err(error.into()),
        };
        let mut input = BufReader::new(file);
        let mut reader = XmlReader::new(&mut input);
        match reader.next_tok()? {
            Tok::Start(start) if start.name == "IncomingLists" => {}
            Tok::Start(start) => {
                return Err(XmlError(format!("unexpected root <{}>", start.name)).into())
            }
            Tok::Eof => return Err(XmlError("empty incoming list file".into()).into()),
            token => {
                return Err(XmlError(format!(
                    "unexpected token before incoming-list root: {token:?}"
                ))
                .into())
            }
        }
        let mut lists = Vec::new();
        loop {
            match reader.next_tok()? {
                Tok::Start(start) if start.name == "Item" => {
                    match ComicListItem::from_start(&start, &mut reader)? {
                        ComicListItem::Smart(list) => lists.push(list),
                        _ => {
                            return Err(XmlError(
                                "IncomingLists.xml supports smart lists only".into(),
                            )
                            .into())
                        }
                    }
                }
                Tok::Start(start) => reader.skip_element(&start.name)?,
                Tok::End(name) if name == "IncomingLists" => break,
                Tok::Eof => return Err(XmlError("eof before </IncomingLists>".into()).into()),
                _ => {}
            }
        }
        let result = Self { lists };
        result.validate_ids()?;
        Ok(result)
    }

    pub fn save(&self, paths: &Paths) -> Result<(), IncomingListError> {
        self.save_to(&incoming_lists_file(paths))
    }

    pub fn save_to(&self, path: &Path) -> Result<(), IncomingListError> {
        self.validate_ids()?;
        let mut output = Vec::new();
        let mut emitter = Emitter::new(&mut output)?;
        emitter.root("IncomingLists")?;
        for list in &self.lists {
            ComicListItem::Smart(list.clone()).write_xml_with_optional_display(&mut emitter)?;
        }
        emitter.end()?;
        emitter.finish()?;
        cr_core::durable::durable_replace(path, &output).map_err(IncomingListError::Io)
    }

    /// Adds a user list. A new ID is assigned once when the caller did not supply one.
    pub fn add(&mut self, mut list: SmartListItem) -> CrGuid {
        if list.base.id.is_empty() {
            loop {
                list.base.id = CrGuid::new_random();
                if !self.lists.iter().any(|item| item.base.id == list.base.id) {
                    break;
                }
            }
        }
        let id = list.base.id;
        self.lists.push(list);
        id
    }

    /// Evaluates a user list against Incoming books only.
    pub fn evaluate<'a>(
        &self,
        id: CrGuid,
        catalog: &'a IncomingCatalog,
    ) -> Result<Vec<&'a ComicBook>, IncomingListError> {
        self.validate_ids()?;
        let all: Vec<&ComicBook> = catalog.books.iter().collect();
        self.evaluate_inner(id, &all, &mut Vec::new())
    }

    fn validate_ids(&self) -> Result<(), IncomingListError> {
        let mut ids = HashSet::new();
        for list in &self.lists {
            if list.base.id.is_empty() {
                return Err(IncomingListError::EmptyId);
            }
            if !ids.insert(list.base.id) {
                return Err(IncomingListError::DuplicateId(list.base.id));
            }
        }
        Ok(())
    }

    fn evaluate_inner<'a>(
        &self,
        id: CrGuid,
        all: &[&'a ComicBook],
        visiting: &mut Vec<CrGuid>,
    ) -> Result<Vec<&'a ComicBook>, IncomingListError> {
        if visiting.contains(&id) {
            return Err(IncomingListError::Evaluation(
                crate::lists::ListEvaluationError::BaseCycle(id),
            ));
        }
        let list = self
            .lists
            .iter()
            .find(|list| list.base.id == id)
            .ok_or(IncomingListError::MissingList(id))?;
        visiting.push(id);
        let base = if list.base_list_id.is_empty() {
            None
        } else if self
            .lists
            .iter()
            .any(|base| base.base.id == list.base_list_id)
        {
            Some(self.evaluate_inner(list.base_list_id, all, visiting)?)
        } else {
            return Err(IncomingListError::Evaluation(
                crate::lists::ListEvaluationError::MissingBase(list.base_list_id),
            ));
        };
        let result = crate::smart_list::evaluate_smart_list_checked(list, all, base.as_deref())
            .map_err(|error| {
                IncomingListError::Evaluation(crate::lists::ListEvaluationError::Matcher(error))
            });
        visiting.pop();
        result
    }
}

/// Moves records under newly assigned Incoming roots into the separate catalog.
pub fn transfer_new_incoming_records(
    database: &mut cr_core::database::comic_database::ComicDatabase,
    catalog: &mut IncomingCatalog,
    old_config: &IncomingConfig,
    roles: &[FolderRole],
) -> usize {
    let indexes: HashSet<usize> = conversion_indexes(&database.books, old_config, roles)
        .into_iter()
        .collect();
    let count = indexes.len();
    let mut retained = Vec::with_capacity(database.books.len() - count);
    for (index, book) in std::mem::take(&mut database.books).into_iter().enumerate() {
        if indexes.contains(&index) {
            catalog.books.push(book);
        } else {
            retained.push(book);
        }
    }
    database.books = retained;
    count
}

/// Returns the normalized shadow identity, or `None` for an empty series.
pub fn incoming_identity(book: &ComicBook) -> Option<IncomingIdentity> {
    let proposed = book_view::proposed_cached(book);
    let series = normalize_series(book_view::shadow_series(book, &proposed));
    if series.is_empty() {
        return None;
    }
    Some(IncomingIdentity {
        series,
        volume: book_view::shadow_volume(book, &proposed),
        format: book_view::shadow_format(book, &proposed).to_ascii_lowercase(),
        language: book.info.language_iso.to_ascii_lowercase(),
    })
}

/// Classifies all incoming records against the main library and cached gaps.
pub fn classify_incoming(
    incoming: &[ComicBook],
    library: &[ComicBook],
    external_gaps: &ExternalGapCache,
) -> Vec<IncomingClassification> {
    let all: Vec<&ComicBook> = incoming.iter().chain(library).collect();
    let mut duplicate = vec![false; incoming.len()];
    let mut library_duplicate = vec![false; incoming.len()];
    let mut incoming_duplicate = vec![false; incoming.len()];
    for group in grouped_duplicate_indexes(&all) {
        let has_library_match = group.iter().any(|index| *index >= incoming.len());
        let incoming_match_count = group
            .iter()
            .filter(|index| **index < incoming.len())
            .count();
        for index in group.into_iter().filter(|index| *index < incoming.len()) {
            duplicate[index] = true;
            library_duplicate[index] = has_library_match;
            incoming_duplicate[index] = incoming_match_count > 1;
        }
    }

    let library_identities: HashSet<_> = library.iter().filter_map(incoming_identity).collect();
    let mut owned_numbers: HashMap<IncomingIdentity, Vec<f32>> = HashMap::new();
    for book in library {
        if let (Some(identity), Some(number)) = (incoming_identity(book), issue_number(book)) {
            owned_numbers
                .entry(identity)
                .or_default()
                .push(number.value());
        }
    }

    incoming
        .iter()
        .enumerate()
        .map(|(index, book)| {
            let identity = incoming_identity(book);
            let number = issue_number(book);
            let new_series = identity
                .as_ref()
                .is_some_and(|value| !library_identities.contains(value));
            let gap_fill = match (identity.as_ref(), number) {
                (Some(identity), Some(number)) => {
                    external_gaps
                        .get(identity)
                        .is_some_and(|gaps| gaps.contains(&number))
                        || fills_internal_gap(
                            number.value(),
                            owned_numbers
                                .get(identity)
                                .map(Vec::as_slice)
                                .unwrap_or(&[]),
                        )
                }
                _ => false,
            };
            let needs_review = !duplicate[index] && !gap_fill && !new_series;
            IncomingClassification {
                index,
                duplicate: duplicate[index],
                library_duplicate: library_duplicate[index],
                incoming_duplicate: incoming_duplicate[index],
                gap_fill,
                new_series,
                needs_review,
            }
        })
        .collect()
}

fn issue_number(book: &ComicBook) -> Option<IssueNumber> {
    let proposed = book_view::proposed_cached(book);
    IssueNumber::parse(book_view::shadow_number(book, &proposed))
}

fn fills_internal_gap(number: f32, owned: &[f32]) -> bool {
    if owned.len() < 2 || owned.contains(&number) {
        return false;
    }
    let min = owned.iter().copied().reduce(f32::min).unwrap_or(number);
    let max = owned.iter().copied().reduce(f32::max).unwrap_or(number);
    number > min && number < max
}

fn normalize_series(text: &str) -> String {
    const SEPARATORS: [char; 15] = [
        ' ', '\t', '\n', '\r', '-', '~', ',', '.', ';', ':', '/', '\\', '\'', '\u{b4}', '`',
    ];
    const ARTICLES: [&str; 8] = ["the", "der", "die", "das", "le", "la", "les", "l'"];
    text.split(SEPARATORS)
        .filter(|part| {
            !part.is_empty()
                && !ARTICLES
                    .iter()
                    .any(|article| part.eq_ignore_ascii_case(article))
        })
        .collect::<String>()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::database::comic_database::{save as save_comic_db, ComicDatabase};
    use cr_core::database::list_items::{
        ComicBookMatcher, ComicListItem, IdListItem, ListItemBase, SmartListItem, ValueMatcher,
    };
    use cr_core::xml::scalar::CrGuid;

    fn book(id: &str, series: &str, number: &str) -> ComicBook {
        let mut book = ComicBook {
            id: CrGuid::parse(id).unwrap(),
            file_path: format!("/incoming/{id}.cbz"),
            enable_proposed: false,
            ..ComicBook::default()
        };
        book.info.series = series.into();
        book.info.number = number.into();
        book.info.volume = 1;
        book.info.format = "Digital".into();
        book.info.language_iso = "en".into();
        book
    }

    fn id(last: u8) -> String {
        format!("00000000-0000-0000-0000-{last:012}")
    }

    fn temp_paths(name: &str) -> Paths {
        let root = std::env::temp_dir().join(format!(
            "comicrust-incoming-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        Paths::from_xdg_root(&root)
    }

    fn smart_list(name: &str, matcher_type: &str, op: i32, value: &str) -> SmartListItem {
        SmartListItem {
            base: ListItemBase {
                id: CrGuid::new_random(),
                name: Some(name.into()),
                ..Default::default()
            },
            matchers: vec![ComicBookMatcher::Value(ValueMatcher {
                type_name: matcher_type.into(),
                match_operator: op,
                match_value: value.into(),
                ..Default::default()
            })],
            ..Default::default()
        }
    }

    #[test]
    fn defaults_are_empty() {
        assert_eq!(
            IncomingConfig::default(),
            IncomingConfig {
                incoming_folders: Vec::new(),
                last_organizer_profile: String::new(),
                find_in_incoming_profile: String::new(),
            }
        );
    }

    #[test]
    fn serde_uses_the_plugin_field_names() {
        let config: IncomingConfig = toml::from_str(
            r#"
incoming_folders = ["/data/Incoming"]
last_organizer_profile = "Move to Library"
find_in_incoming_profile = "Missing Issues Move"
"#,
        )
        .expect("incoming config parses");
        assert_eq!(config.incoming_folders, ["/data/Incoming"]);
        assert_eq!(config.last_organizer_profile, "Move to Library");
        assert_eq!(config.find_in_incoming_profile, "Missing Issues Move");

        let text = toml::to_string(&config).expect("incoming config serializes");
        assert!(text.contains("incoming_folders = [\"/data/Incoming\"]"));
        assert!(text.contains("last_organizer_profile = \"Move to Library\""));
        assert!(text.contains("find_in_incoming_profile = \"Missing Issues Move\""));
    }

    #[test]
    fn old_config_defaults_the_find_in_incoming_profile() {
        let config: IncomingConfig = toml::from_str(
            r#"
incoming_folders = []
last_organizer_profile = "Move to Library"
"#,
        )
        .expect("old incoming config parses");
        assert!(config.find_in_incoming_profile.is_empty());
    }

    #[test]
    fn missing_issue_matching_uses_series_volume_and_number_only() {
        let mut missing_one = book(&id(40), "The Alpha-Series", "01");
        missing_one.info.format.clear();
        missing_one.info.language_iso.clear();
        let mut missing_two = book(&id(41), "Alpha Series", "2");
        missing_two.info.volume = 2;

        let mut first = book(&id(1), "alpha series", "1");
        first.info.format = "Print".into();
        first.info.language_iso = "fr".into();
        let second = book(&id(2), "Alpha.Series", "001");
        let mut wrong_volume = book(&id(3), "Alpha Series", "2");
        wrong_volume.info.volume = 1;

        let matches = find_missing_incoming_matches(
            &[missing_one, missing_two],
            &[first, second, wrong_volume],
        );
        assert_eq!(matches[0].incoming_indexes, [0, 1]);
        assert!(matches[1].incoming_indexes.is_empty());
    }

    #[test]
    fn incoming_match_is_component_aware_and_separator_neutral() {
        let config = IncomingConfig {
            incoming_folders: vec!["C:\\Comics\\Incoming\\".into()],
            ..Default::default()
        };

        assert!(config.is_incoming_path("c:/comics/incoming/Series/book.cbz"));
        assert!(!config.is_incoming_path("c:/comics/incoming-old/book.cbz"));
        assert!(!config.is_incoming_path("c:/comics/incoming"));
    }

    #[test]
    fn any_incoming_role_also_has_the_monitored_role() {
        let config = IncomingConfig {
            incoming_folders: vec!["/staging/incoming".into()],
            ..Default::default()
        };

        let path = "/staging/incoming/series/book.cbz";
        assert!(config.is_incoming_path(path));
        assert!(config.is_monitored_path(path));
        assert!(!config.is_monitored_path("/library/series/book.cbz"));
    }

    #[test]
    fn overlap_validation_is_component_aware() {
        let roles = vec![
            FolderRole {
                folder: "/books/library".into(),
                incoming: false,
                watch: false,
            },
            FolderRole {
                folder: "/books/library-new".into(),
                incoming: true,
                watch: true,
            },
        ];
        assert!(validate_folder_roots(&roles).is_ok());

        let nested = vec![
            roles[0].clone(),
            FolderRole {
                folder: "/BOOKS/LIBRARY/incoming/".into(),
                incoming: true,
                watch: true,
            },
        ];
        assert!(validate_folder_roots(&nested).is_err());

        let exact = vec![
            roles[0].clone(),
            FolderRole {
                folder: "\\books\\library\\".into(),
                incoming: true,
                watch: true,
            },
        ];
        assert!(validate_folder_roots(&exact).is_err());
    }

    #[test]
    fn conversion_selects_only_books_under_new_incoming_roots() {
        let old = IncomingConfig {
            incoming_folders: vec!["/incoming/old".into()],
            ..Default::default()
        };
        let roles = vec![
            FolderRole {
                folder: "/incoming/old".into(),
                incoming: true,
                watch: true,
            },
            FolderRole {
                folder: "/library/new".into(),
                incoming: true,
                watch: true,
            },
        ];
        let mut books = vec![
            book(&id(1), "A", "1"),
            book(&id(2), "B", "1"),
            book(&id(3), "C", "1"),
        ];
        books[0].file_path = "/library/new/A.cbz".into();
        books[1].file_path = "/library/newer/B.cbz".into();
        books[2].file_path = "/incoming/old/C.cbz".into();

        assert_eq!(conversion_indexes(&books, &old, &roles), vec![0]);
    }

    #[test]
    fn conversion_moves_the_complete_record_and_keeps_list_ids() {
        let roles = vec![FolderRole {
            folder: "/library/new".into(),
            incoming: true,
            watch: true,
        }];
        let mut moved = book(&id(1), "A", "1");
        moved.file_path = "/library/new/A.cbz".into();
        moved.info.summary = "Full metadata".into();
        moved.book_notes = "Review this".into();
        moved.rating = 4.5;
        let moved_id = moved.id;
        let mut database = ComicDatabase {
            books: vec![moved.clone()],
            comic_lists: vec![ComicListItem::IdList(IdListItem {
                book_ids: vec![moved_id],
                ..Default::default()
            })],
            ..Default::default()
        };
        let lists_before = database.comic_lists.clone();
        let mut catalog = IncomingCatalog::default();

        assert_eq!(
            transfer_new_incoming_records(
                &mut database,
                &mut catalog,
                &IncomingConfig::default(),
                &roles,
            ),
            1
        );
        assert!(database.books.is_empty());
        assert_eq!(catalog.books, vec![moved]);
        assert_eq!(database.comic_lists, lists_before);
    }

    #[test]
    fn unresolved_records_block_folder_and_role_removal() {
        let old = vec![FolderRole {
            folder: "/incoming/review".into(),
            incoming: true,
            watch: true,
        }];
        let mut unresolved = book(&id(1), "A", "1");
        unresolved.file_path = "/incoming/review/A.cbz".into();

        assert!(validate_unresolved_removals(&old, &[], &[unresolved.clone()]).is_err());
        assert!(validate_unresolved_removals(
            &old,
            &[FolderRole {
                folder: "/incoming/review".into(),
                incoming: false,
                watch: true,
            }],
            &[unresolved]
        )
        .is_err());
        assert!(validate_unresolved_removals(&old, &[], &[]).is_ok());
    }

    #[test]
    fn atomic_roundtrip_preserves_ids_and_full_fields() {
        let paths = temp_paths("roundtrip");
        let mut full = book(&id(1), "The Alpha", "2");
        full.info.summary = "Complete metadata".into();
        full.book_notes = "Catalog state".into();
        full.rating = 4.5;
        let catalog = IncomingCatalog { books: vec![full] };

        catalog.save(&paths).unwrap();
        let loaded = IncomingCatalog::load(&paths).unwrap();

        assert_eq!(loaded, catalog);
        assert!(incoming_file(&paths).is_file());
    }

    #[test]
    fn missing_catalog_loads_as_empty() {
        let paths = temp_paths("missing");

        assert_eq!(
            IncomingCatalog::load(&paths).unwrap(),
            IncomingCatalog::default()
        );
    }

    #[test]
    fn malformed_catalog_returns_an_error() {
        let paths = temp_paths("malformed");
        let path = incoming_file(&paths);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"<IncomingDatabase><Book>").unwrap();

        assert!(matches!(
            IncomingCatalog::load(&paths),
            Err(IncomingError::Xml(_))
        ));
    }

    #[test]
    fn saving_incoming_does_not_mutate_comic_db() {
        let paths = temp_paths("separate");
        let comic_db_path = cr_core::paths::database_file(&paths);
        save_comic_db(&ComicDatabase::default(), &comic_db_path).unwrap();
        let before = std::fs::read(&comic_db_path).unwrap();

        IncomingCatalog {
            books: vec![book(&id(1), "Alpha", "1")],
        }
        .save(&paths)
        .unwrap();

        assert_eq!(std::fs::read(comic_db_path).unwrap(), before);
    }

    #[test]
    fn incoming_lists_roundtrip_separately_with_stable_ids() {
        let paths = temp_paths("lists-roundtrip");
        let comic_db_path = cr_core::paths::database_file(&paths);
        save_comic_db(&ComicDatabase::default(), &comic_db_path).unwrap();
        let comic_db_before = std::fs::read(&comic_db_path).unwrap();
        let mut lists = IncomingLists::default();
        let id = lists.add(smart_list("Alpha", "ComicBookSeriesMatcher", 0, "Alpha"));

        lists.save(&paths).unwrap();
        let loaded = IncomingLists::load(&paths).unwrap();

        assert_eq!(loaded, lists);
        assert_eq!(loaded.lists[0].base.id, id);
        assert!(incoming_lists_file(&paths).is_file());
        assert_eq!(std::fs::read(comic_db_path).unwrap(), comic_db_before);
        let xml = std::fs::read_to_string(incoming_lists_file(&paths)).unwrap();
        assert!(xml.contains("<IncomingLists"));
        assert!(xml.contains("xsi:type=\"ComicSmartListItem\""));
        assert!(!xml.contains("<Display"));
    }

    #[test]
    fn incoming_list_matchers_use_only_incoming_candidates_and_statistics() {
        let alpha_one = book(&id(1), "Alpha", "1");
        let alpha_two = book(&id(2), "Alpha", "2");
        let beta = book(&id(3), "Beta", "1");
        let catalog = IncomingCatalog {
            books: vec![alpha_one, alpha_two, beta],
        };
        let duplicates = smart_list("Duplicates", "ComicBookDuplicateMatcher", 0, "");
        let duplicate_id = duplicates.base.id;
        let count = smart_list("Series count", "SmartListSeriesCountMatcher", 1, "1");
        let count_id = count.base.id;
        let lists = IncomingLists {
            lists: vec![duplicates, count],
        };

        // No Incoming duplicates exist. A Library book is not a candidate.
        assert!(lists.evaluate(duplicate_id, &catalog).unwrap().is_empty());
        let result = lists.evaluate(count_id, &catalog).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|book| book.info.series == "Alpha"));
    }

    #[test]
    fn incoming_lists_support_bases_and_report_invalid_graphs() {
        let catalog = IncomingCatalog {
            books: vec![
                book(&id(1), "Alpha", "1"),
                book(&id(2), "Alpha", "2"),
                book(&id(3), "Beta", "1"),
            ],
        };
        let alpha = smart_list("Alpha", "ComicBookSeriesMatcher", 0, "Alpha");
        let alpha_id = alpha.base.id;
        let mut outside = SmartListItem {
            base: ListItemBase {
                id: CrGuid::new_random(),
                name: Some("Outside Alpha".into()),
                ..Default::default()
            },
            base_list_id: alpha_id,
            not_in_base_list: true,
            ..Default::default()
        };
        let outside_id = outside.base.id;
        let lists = IncomingLists {
            lists: vec![alpha.clone(), outside.clone()],
        };
        let result = lists.evaluate(outside_id, &catalog).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].info.series, "Beta");

        outside.base_list_id = CrGuid::new_random();
        let missing = IncomingLists {
            lists: vec![outside.clone()],
        };
        assert!(matches!(
            missing.evaluate(outside_id, &catalog),
            Err(IncomingListError::Evaluation(
                crate::lists::ListEvaluationError::MissingBase(_)
            ))
        ));

        outside.base_list_id = alpha_id;
        let mut cyclic_alpha = alpha;
        cyclic_alpha.base_list_id = outside_id;
        let cyclic = IncomingLists {
            lists: vec![cyclic_alpha, outside],
        };
        assert!(matches!(
            cyclic.evaluate(outside_id, &catalog),
            Err(IncomingListError::Evaluation(
                crate::lists::ListEvaluationError::BaseCycle(_)
            ))
        ));
    }

    #[test]
    fn incoming_unknown_matcher_is_an_explicit_error() {
        let invalid = smart_list("Invalid", "ComicBookNoSuchMatcher", 0, "");
        let list_id = invalid.base.id;
        let lists = IncomingLists {
            lists: vec![invalid],
        };
        let catalog = IncomingCatalog {
            books: vec![book(&id(1), "Alpha", "1")],
        };

        assert!(matches!(
            lists.evaluate(list_id, &catalog),
            Err(IncomingListError::Evaluation(
                crate::lists::ListEvaluationError::Matcher(
                    crate::smart_list::SmartListError::UnknownMatcher(_)
                )
            ))
        ));
    }

    #[test]
    fn classifications_cover_each_group_and_overlap() {
        let library = vec![book(&id(10), "The Alpha", "1"), book(&id(11), "Alpha", "3")];
        let incoming = vec![
            book(&id(1), "Alpha", "1"),
            book(&id(2), "Alpha", "2"),
            book(&id(3), "Beta", "1"),
            book(&id(4), "Alpha", "9"),
            book(&id(5), "Beta", "1"),
        ];

        let result = classify_incoming(&incoming, &library, &ExternalGapCache::new());

        assert_eq!(result.len(), incoming.len());
        assert!(result[0].duplicate);
        assert!(result[0].library_duplicate);
        assert!(!result[0].incoming_duplicate);
        assert!(result[1].gap_fill);
        assert!(result[2].new_series);
        assert!(result[2].duplicate);
        assert!(!result[2].library_duplicate);
        assert!(result[2].incoming_duplicate);
        assert!(result[4].new_series);
        assert!(result[4].duplicate);
        assert!(!result[4].library_duplicate);
        assert!(result[4].incoming_duplicate);
        assert!(result[3].needs_review);
        assert!(!result[0].needs_review);
    }

    #[test]
    fn duplicate_classification_can_match_both_catalogs() {
        let library = vec![book(&id(10), "Alpha", "1")];
        let incoming = vec![book(&id(1), "Alpha", "1"), book(&id(2), "Alpha", "1")];

        let result = classify_incoming(&incoming, &library, &ExternalGapCache::new());

        for classification in result {
            assert!(classification.duplicate);
            assert!(classification.library_duplicate);
            assert!(classification.incoming_duplicate);
        }
    }

    #[test]
    fn malformed_numbers_are_not_gaps() {
        let library = vec![book(&id(10), "Alpha", "1"), book(&id(11), "Alpha", "3")];
        let incoming = vec![book(&id(1), "Alpha", "not-an-issue")];

        let result = classify_incoming(&incoming, &library, &ExternalGapCache::new());

        assert!(!result[0].gap_fill);
        assert!(result[0].needs_review);
    }

    #[test]
    fn external_cache_can_supply_a_gap_outside_the_owned_range() {
        let library = vec![book(&id(10), "The Alpha", "1")];
        let incoming = vec![book(&id(1), "Alpha", "20")];
        let identity = incoming_identity(&incoming[0]).unwrap();
        let mut gaps = ExternalGapCache::new();
        gaps.insert(identity, vec![IssueNumber::parse("20").unwrap()]);

        let result = classify_incoming(&incoming, &library, &gaps);

        assert!(result[0].gap_fill);
        assert!(!result[0].needs_review);
    }

    #[test]
    fn empty_identity_is_not_a_new_series() {
        let incoming = vec![book(&id(1), "", "1")];
        let result = classify_incoming(&incoming, &[], &ExternalGapCache::new());

        assert!(!result[0].new_series);
        assert!(result[0].needs_review);
    }
}
