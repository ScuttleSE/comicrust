//! `ComicBook` — ComicInfo plus the ~30 book state members, with the
//! exact attribute/element layout of the `<Book>` entry in ComicDb.xml.

use crate::model::bitmap_adjustment::{BitmapAdjustment, ExtraSyncInformation};
use crate::model::comic_info::{read_info_elem, ComicInfo};
use crate::model::enums::YesNo;
use crate::xml::reader::{XmlError, XmlResult};
use crate::xml::scalar::{net_f32, CrDateTime, CrGuid};
use crate::xml::{Emitter, Start, Tok};
use std::io::Write;

#[derive(Clone, Debug, PartialEq)]
pub struct ComicBook {
    pub info: ComicInfo,
    /// Attribute; omitted when `Guid.Empty` (ShouldSerializeId).
    pub id: CrGuid,
    pub checked: bool,
    pub file_path: String,
    pub is_dynamic_source: bool,

    pub added_time: CrDateTime,
    pub released_time: CrDateTime,
    pub opened_time: CrDateTime,
    pub opened_count: i32,
    pub current_page: i32,
    pub last_page_read: i32,
    pub rating: f32,
    pub color_adjustment: BitmapAdjustment,
    pub enable_proposed: bool,
    pub series_complete: YesNo,
    pub enable_dynamic_update: bool,
    pub last_opened_from_list_id: CrGuid,
    pub comic_info_is_dirty: bool,
    pub comic_book_is_dirty: bool,
    pub file_size: i64,
    /// `FileIsMissing`; serialized as `<Missing>` (false → not written).
    pub file_is_missing: bool,
    pub file_modified_time: CrDateTime,
    pub file_creation_time: CrDateTime,
    pub custom_thumbnail_key: Option<String>,
    pub book_price: f32,
    pub book_age: String,
    pub book_condition: String,
    pub book_store: String,
    pub book_owner: String,
    pub book_collection_status: String,
    pub book_notes: String,
    pub book_location: String,
    pub isbn: String,
    pub new_pages: i32,
    pub extra_sync_information: Option<ExtraSyncInformation>,
    pub custom_values_store: String,
}

impl Default for ComicBook {
    fn default() -> Self {
        ComicBook {
            info: ComicInfo::default(),
            id: CrGuid::EMPTY,
            checked: true,
            file_path: String::new(),
            is_dynamic_source: false,
            added_time: CrDateTime::min_value(),
            released_time: CrDateTime::min_value(),
            opened_time: CrDateTime::min_value(),
            opened_count: 0,
            current_page: 0,
            last_page_read: 0,
            rating: 0.0,
            color_adjustment: BitmapAdjustment::default(),
            enable_proposed: true,
            series_complete: YesNo::Unknown,
            enable_dynamic_update: true,
            last_opened_from_list_id: CrGuid::EMPTY,
            comic_info_is_dirty: false,
            comic_book_is_dirty: false,
            file_size: -1,
            file_is_missing: false,
            file_modified_time: CrDateTime::min_value(),
            file_creation_time: CrDateTime::min_value(),
            custom_thumbnail_key: None,
            book_price: -1.0,
            book_age: String::new(),
            book_condition: String::new(),
            book_store: String::new(),
            book_owner: String::new(),
            book_collection_status: String::new(),
            book_notes: String::new(),
            book_location: String::new(),
            isbn: String::new(),
            new_pages: 0,
            extra_sync_information: None,
            custom_values_store: String::new(),
        }
    }
}

impl ComicBook {
    pub fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("Book")?;
        self.write_body(e)?;
        e.end()
    }

    /// Attributes and child elements, shared by the `<Book>` database
    /// entry and the standalone `ComicBook.xml` root.
    fn write_body<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        // Attributes: Id, Checked, File, IsDynamicSource
        if !self.id.is_empty() {
            e.attr("Id", &self.id.to_d_string())?;
        }
        if !self.checked {
            e.attr("Checked", "false")?;
        }
        if !self.file_path.is_empty() {
            e.attr("File", &self.file_path)?;
        }
        if self.is_dynamic_source {
            e.attr("IsDynamicSource", "true")?;
        }
        // ComicInfo members first (base class), including Pages.
        self.info.write_xml(e)?;
        // Book elements in declaration order.
        if !self.added_time.is_min_value() {
            e.text_elem("Added", &self.added_time.to_xml())?;
        }
        if !self.released_time.is_min_value() {
            // Getter truncates to date (`.DateOnly()`).
            e.text_elem("Released", &self.released_time.date_only().to_xml())?;
        }
        if !self.opened_time.is_min_value() {
            e.text_elem("Opened", &self.opened_time.to_xml())?;
        }
        if self.opened_count != 0 {
            e.text_elem("OpenCount", &self.opened_count.to_string())?;
        }
        if self.current_page != 0 {
            e.text_elem("CurrentPage", &self.current_page.to_string())?;
        }
        if self.last_page_read != 0 {
            e.text_elem("LastPageRead", &self.last_page_read.to_string())?;
        }
        if self.rating != 0.0 {
            e.text_elem("Rating", &net_f32(self.rating))?;
        }
        if !self.color_adjustment.is_empty() {
            self.color_adjustment.write_xml(e)?;
        }
        if !self.enable_proposed {
            e.text_elem("EnableProposed", "false")?;
        }
        if self.series_complete != YesNo::Unknown {
            e.text_elem("SeriesComplete", &self.series_complete.to_xml())?;
        }
        if !self.enable_dynamic_update {
            e.text_elem("EnableDynamicUpdate", "false")?;
        }
        if !self.last_opened_from_list_id.is_empty() {
            e.text_elem(
                "LastOpenedFromListId",
                &self.last_opened_from_list_id.to_d_string(),
            )?;
        }
        if self.comic_info_is_dirty {
            e.text_elem("ComicInfoIsDirty", "true")?;
        }
        if self.comic_book_is_dirty {
            e.text_elem("ComicBookIsDirty", "true")?;
        }
        if self.file_size != -1 {
            e.text_elem("FileSize", &self.file_size.to_string())?;
        }
        if self.file_is_missing {
            e.text_elem("Missing", "true")?;
        }
        if !self.file_modified_time.is_min_value() {
            e.text_elem("FileModifiedTime", &self.file_modified_time.to_xml())?;
        }
        if !self.file_creation_time.is_min_value() {
            e.text_elem("FileCreationTime", &self.file_creation_time.to_xml())?;
        }
        if let Some(k) = &self.custom_thumbnail_key {
            e.text_elem("CustomThumbnailKey", k)?;
        }
        if self.book_price != -1.0 {
            e.text_elem("BookPrice", &net_f32(self.book_price))?;
        }
        for (name, v) in [
            ("BookAge", &self.book_age),
            ("BookCondition", &self.book_condition),
            ("BookStore", &self.book_store),
            ("BookOwner", &self.book_owner),
            ("BookCollectionStatus", &self.book_collection_status),
            ("BookNotes", &self.book_notes),
            ("BookLocation", &self.book_location),
            ("ISBN", &self.isbn),
        ] {
            if !v.is_empty() {
                e.text_elem(name, v)?;
            }
        }
        if self.new_pages != 0 {
            e.text_elem("NewPages", &self.new_pages.to_string())?;
        }
        if let Some(x) = &self.extra_sync_information {
            x.write_xml(e)?;
        }
        if !self.custom_values_store.is_empty() {
            e.text_elem("CustomValuesStore", &self.custom_values_store)?;
        }
        Ok(())
    }

    /// `ComicBook.Serialize` (ComicBook.cs:2743) — the sidecar
    /// document: file-derived and library-related fields are stripped
    /// on a clone so the caller's object is not modified.
    pub fn serialize_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut cb = self.clone();
        cb.id = CrGuid::EMPTY;
        cb.file_path = String::new();
        cb.file_modified_time = CrDateTime::min_value();
        cb.file_creation_time = CrDateTime::min_value();
        cb.file_size = -1;
        cb.last_opened_from_list_id = CrGuid::EMPTY;
        cb.custom_thumbnail_key = None;
        cb.comic_info_is_dirty = false;
        cb.comic_book_is_dirty = false;
        cb.extra_sync_information = None;
        cb.new_pages = 0;
        cb.is_dynamic_source = false;
        cb.enable_dynamic_update = true;
        cb.serialize_full_bytes()
    }

    /// `ComicBook.SerializeFull` — the whole object.
    pub fn serialize_full_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut e = Emitter::new(&mut out)?;
        e.root("ComicBook")?;
        self.write_body(&mut e)?;
        e.end()?;
        e.finish()?;
        Ok(out)
    }

    /// `ComicBook.IsSameContent` — `ComicInfo.IsSameContent` plus the
    /// book-state chain from ComicBook.cs:2697 (file-derived fields
    /// excluded, matching the C# commented-out comparisons).
    pub fn is_same_content(&self, other: &ComicBook, with_pages: bool) -> bool {
        self.info.is_same_content(&other.info, with_pages)
            && self.added_time == other.added_time
            && self.released_time == other.released_time
            && self.opened_time == other.opened_time
            && self.opened_count == other.opened_count
            && self.current_page == other.current_page
            && self.last_page_read == other.last_page_read
            && self.rating == other.rating
            && self.color_adjustment == other.color_adjustment
            && self.enable_proposed == other.enable_proposed
            && self.series_complete == other.series_complete
            && self.checked == other.checked
            && self.custom_values_store == other.custom_values_store
            && self.book_store == other.book_store
            && self.book_price == other.book_price
            && self.isbn == other.isbn
            && self.book_age == other.book_age
            && self.book_condition == other.book_condition
            && self.book_owner == other.book_owner
            && self.book_location == other.book_location
            && self.book_collection_status == other.book_collection_status
            && self.book_notes == other.book_notes
    }

    /// Parses `<Book>` (attrs consumed, children follow).
    pub fn read_xml(start: &Start, r: &mut crate::xml::XmlReader<'_>) -> XmlResult<ComicBook> {
        let mut b = ComicBook::default();
        read_attrs(start, &mut b)?;
        read_children(&mut b, "Book", r)?;
        Ok(b)
    }

    /// Parses a standalone `ComicBook.xml` document (root
    /// `<ComicBook>`).
    pub fn parse_root(r: &mut crate::xml::XmlReader<'_>) -> XmlResult<ComicBook> {
        loop {
            match r.next_tok()? {
                Tok::Start(s) if s.name == "ComicBook" => {
                    let mut b = ComicBook::default();
                    read_attrs(&s, &mut b)?;
                    read_children(&mut b, "ComicBook", r)?;
                    return Ok(b);
                }
                Tok::Eof => return Err(XmlError("no ComicBook root".into())),
                _ => {}
            }
        }
    }
}

fn read_attrs(start: &Start, b: &mut ComicBook) -> XmlResult<()> {
    for (k, v) in &start.attrs {
        match k.as_str() {
            "Id" => b.id = CrGuid::parse(v)?,
            "Checked" => b.checked = parse_bool(v, "Checked")?,
            "File" => b.file_path = v.clone(),
            "IsDynamicSource" => b.is_dynamic_source = parse_bool(v, "IsDynamicSource")?,
            _ => {}
        }
    }
    Ok(())
}

fn read_children(b: &mut ComicBook, end: &str, r: &mut crate::xml::XmlReader<'_>) -> XmlResult<()> {
    loop {
        match r.next_tok()? {
            Tok::Eof => return Err(XmlError(format!("eof in {end}"))),
            Tok::End(n) if n == end => return Ok(()),
            Tok::Start(s) => {
                if read_info_elem(r, &s, &mut b.info)? {
                    continue;
                }
                match s.name.as_str() {
                    "Added" => b.added_time = read_dt(r, "Added")?,
                    "Released" => b.released_time = read_dt(r, "Released")?,
                    "Opened" => b.opened_time = read_dt(r, "Opened")?,
                    "OpenCount" => b.opened_count = read_i32(r, "OpenCount")?,
                    "CurrentPage" => b.current_page = read_i32(r, "CurrentPage")?,
                    "LastPageRead" => b.last_page_read = read_i32(r, "LastPageRead")?,
                    "Rating" => {
                        let v = r.text_content("Rating")?;
                        b.rating = v
                            .trim()
                            .parse()
                            .map_err(|_| XmlError(format!("bad Rating: {v}")))?;
                        if !(0.0..=5.0).contains(&b.rating) {
                            return Err(XmlError(format!("Rating out of range: {v}")));
                        }
                    }
                    "ColorAdjustment" => {
                        b.color_adjustment = read_color_adjustment(r)?;
                    }
                    "EnableProposed" => b.enable_proposed = parse_bool_v(r, "EnableProposed")?,
                    "SeriesComplete" => {
                        let v = r.text_content("SeriesComplete")?;
                        b.series_complete = YesNo::from_xml(&v)
                            .ok_or_else(|| XmlError(format!("bad YesNo: {v}")))?;
                    }
                    "EnableDynamicUpdate" => {
                        b.enable_dynamic_update = parse_bool_v(r, "EnableDynamicUpdate")?
                    }
                    "LastOpenedFromListId" => {
                        let v = r.text_content("LastOpenedFromListId")?;
                        b.last_opened_from_list_id = CrGuid::parse(&v)?;
                    }
                    "ComicInfoIsDirty" => {
                        b.comic_info_is_dirty = parse_bool_v(r, "ComicInfoIsDirty")?
                    }
                    "ComicBookIsDirty" => {
                        b.comic_book_is_dirty = parse_bool_v(r, "ComicBookIsDirty")?
                    }
                    "Missing" => {
                        let v = r.text_content("Missing")?;
                        b.file_is_missing = v.trim() == "true" || v.trim() == "1";
                    }
                    "FileSize" => {
                        let v = r.text_content("FileSize")?;
                        b.file_size = v
                            .trim()
                            .parse()
                            .map_err(|_| XmlError(format!("bad FileSize: {v}")))?;
                    }
                    "FileModifiedTime" => b.file_modified_time = read_dt(r, "FileModifiedTime")?,
                    "FileCreationTime" => b.file_creation_time = read_dt(r, "FileCreationTime")?,
                    "CustomThumbnailKey" => {
                        let v = r.text_content("CustomThumbnailKey")?;
                        b.custom_thumbnail_key = Some(v);
                    }
                    "BookPrice" => {
                        let v = r.text_content("BookPrice")?;
                        b.book_price = v
                            .trim()
                            .parse()
                            .map_err(|_| XmlError(format!("bad BookPrice: {v}")))?;
                    }
                    "BookAge" => b.book_age = r.text_content("BookAge")?,
                    "BookCondition" => b.book_condition = r.text_content("BookCondition")?,
                    "BookStore" => b.book_store = r.text_content("BookStore")?,
                    "BookOwner" => b.book_owner = r.text_content("BookOwner")?,
                    "BookCollectionStatus" => {
                        b.book_collection_status = r.text_content("BookCollectionStatus")?
                    }
                    "BookNotes" => b.book_notes = r.text_content("BookNotes")?,
                    "BookLocation" => b.book_location = r.text_content("BookLocation")?,
                    "ISBN" => b.isbn = r.text_content("ISBN")?,
                    "NewPages" => b.new_pages = read_i32(r, "NewPages")?,
                    "ExtraSyncInformation" => {
                        b.extra_sync_information = Some(read_extra_sync(r)?);
                    }
                    "CustomValuesStore" => {
                        b.custom_values_store = r.text_content("CustomValuesStore")?
                    }
                    // Unknown elements are captured by the inherited
                    // [XmlAnyElement] UnparsedElements.
                    _ => b.info.unparsed_elements.push(r.capture_raw(&s)?),
                }
            }
            Tok::Text(_) => return Err(XmlError(format!("unexpected text in {end}"))),
            _ => {}
        }
    }
}

fn parse_bool(v: &str, ctx: &str) -> XmlResult<bool> {
    match v.trim() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(XmlError(format!("bad bool in {ctx}: {v}"))),
    }
}

fn parse_bool_v(r: &mut crate::xml::XmlReader<'_>, elem: &str) -> XmlResult<bool> {
    let v = r.text_content(elem)?;
    parse_bool(&v, elem)
}

fn read_dt(r: &mut crate::xml::XmlReader<'_>, elem: &str) -> XmlResult<CrDateTime> {
    let v = r.text_content(elem)?;
    CrDateTime::parse(&v).map_err(|e| XmlError(format!("in <{elem}>: {e}")))
}

fn read_i32(r: &mut crate::xml::XmlReader<'_>, elem: &str) -> XmlResult<i32> {
    let v = r.text_content(elem)?;
    v.trim()
        .parse()
        .map_err(|_| XmlError(format!("bad int in <{elem}>: {v}")))
}

fn read_color_adjustment(r: &mut crate::xml::XmlReader<'_>) -> XmlResult<BitmapAdjustment> {
    use crate::model::enums::BitmapAdjustmentOptions;
    let mut a = BitmapAdjustment::default();
    loop {
        match r.next_tok()? {
            Tok::Eof => return Err(XmlError("eof in ColorAdjustment".into())),
            Tok::End(n) if n == "ColorAdjustment" => return Ok(a),
            Tok::Start(s) => {
                let name = s.name.clone();
                let text = r.text_content(&name)?;
                match name.as_str() {
                    "Saturation" => a.saturation = parse_f32(&text, "Saturation")?,
                    "Contrast" => a.contrast = parse_f32(&text, "Contrast")?,
                    "Brightness" => a.brightness = parse_f32(&text, "Brightness")?,
                    "Gamma" => a.gamma = parse_f32(&text, "Gamma")?,
                    "WhitePointArgb" => {
                        a.white_point_argb = text
                            .trim()
                            .parse()
                            .map_err(|_| XmlError(format!("bad WhitePointArgb: {text}")))?
                    }
                    "Options" => {
                        a.options = BitmapAdjustmentOptions::from_xml(text.trim())
                            .ok_or_else(|| XmlError(format!("bad Options: {text}")))?;
                    }
                    "Sharpening" | "Sharpen" => {
                        a.sharpen = text
                            .trim()
                            .parse()
                            .map_err(|_| XmlError(format!("bad Sharpen: {text}")))?
                    }
                    _ => {}
                }
            }
            Tok::Text(_) => {}
            _ => {}
        }
    }
}

fn read_extra_sync(r: &mut crate::xml::XmlReader<'_>) -> XmlResult<ExtraSyncInformation> {
    let mut x = ExtraSyncInformation::default();
    loop {
        match r.next_tok()? {
            Tok::Eof => return Err(XmlError("eof in ExtraSyncInformation".into())),
            Tok::End(n) if n == "ExtraSyncInformation" => return Ok(x),
            Tok::Start(s) => {
                let name = s.name.clone();
                let v = parse_bool_v(r, &name)?;
                match name.as_str() {
                    "ReadingStateChanged" => x.reading_state_changed = v,
                    "InformationChanged" => x.information_changed = v,
                    "BookmarksChanged" => x.bookmarks_changed = v,
                    "PageTypesChanged" => x.page_types_changed = v,
                    "CheckChanged" => x.check_changed = v,
                    "DataChanged" => x.data_changed = v,
                    _ => {}
                }
            }
            Tok::Text(_) => {}
            _ => {}
        }
    }
}

fn parse_f32(v: &str, elem: &str) -> XmlResult<f32> {
    v.trim()
        .parse()
        .map_err(|_| XmlError(format!("bad float in <{elem}>: {v}")))
}

/// The `CustomValuesStore` string codec (`ValuesStore.cs`): comma
/// separated `key=value` pairs, `=`→`&#61;` and `,`→`&#44;` escapes,
/// keys case-insensitive, output sorted by key.
pub mod values_store {
    /// Decodes the store string; malformed fragments (not exactly one
    /// `=`) are silently dropped like the C# parser.
    pub fn decode(s: &str) -> Vec<(String, String)> {
        if s.is_empty() {
            return Vec::new();
        }
        s.split(',')
            .filter_map(|frag| {
                frag.split_once('=')
                    .map(|(k, v)| (unescape(k), unescape(v)))
            })
            .collect()
    }

    /// Encodes pairs sorted by key (case-insensitive, like the C#
    /// `OrderBy` on Windows culture for ASCII keys).
    pub fn encode(pairs: &[(String, String)]) -> String {
        let mut sorted: Vec<&(String, String)> = pairs.iter().collect();
        sorted.sort_by_key(|(k, _)| k.to_lowercase());
        sorted
            .iter()
            .map(|(k, v)| format!("{}={}", escape(k), escape(v)))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn escape(s: &str) -> String {
        s.replace('=', "&#61;").replace(',', "&#44;")
    }

    fn unescape(s: &str) -> String {
        s.replace("&#61;", "=").replace("&#44;", ",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_store_round_trip() {
        let pairs = vec![
            ("a,x".to_string(), "y=z".to_string()),
            ("b".to_string(), "2".to_string()),
        ];
        let s = values_store::encode(&pairs);
        // Output is sorted by key.
        assert_eq!(s, "a&#44;x=y&#61;z,b=2");
        let back = values_store::decode(&s);
        assert_eq!(back, values_store::decode(&values_store::encode(&back)));
        assert_eq!(back, pairs);
    }

    #[test]
    fn values_store_malformed_dropped() {
        // Exactly one '=' per fragment; "=2" and "c=" are kept like C#.
        let back = values_store::decode("a=1,broken,=2,c=");
        assert_eq!(
            back,
            vec![
                ("a".to_string(), "1".to_string()),
                ("".to_string(), "2".to_string()),
                ("c".to_string(), "".to_string()),
            ]
        );
    }
}
