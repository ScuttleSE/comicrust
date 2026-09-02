//! The `ComicLists` tree: list items (polymorphic via `xsi:type`),
//! matchers, and the WatchFolder / BlackList members of ComicDatabase.

use crate::database::display_config::DisplayListConfig;
use crate::model::enums::{
    ComicFolderCombineMode, ComicSmartListLimitSelectionType, ComicSmartListLimitType, MatcherMode,
};
use crate::xml::reader::{XmlError, XmlResult};
use crate::xml::scalar::{CrDateTime, CrGuid};
use crate::xml::{Emitter, Start, Tok};
use std::io::Write;

// ---------- Matchers ----------

/// A leaf matcher. In C# every concrete matcher serializes only the base
/// members; the concrete type is carried by `xsi:type`. The type name is
/// kept verbatim so unknown/third-party matchers round-trip.
#[derive(Clone, Debug, PartialEq)]
pub struct ValueMatcher {
    pub type_name: String,
    pub not: bool,
    pub name: String,
    /// No DefaultValue: always written (empty → `<MatchValue />`).
    pub match_value: String,
    pub match_value_2: String,
    pub match_operator: i32,
    /// `ComicBookStringMatcher.IgnoreCase` ([DefaultValue(true)]: only
    /// written when false, as `IgnoreCase="false"`). Matchers without
    /// the member (numeric, date, ...) never carry it; a `true` here is
    /// indistinguishable from the default and never written.
    pub ignore_case: bool,
    /// `<Option>` child element (`ComicBookAllPropertiesMatcher.Option`
    /// enum name: All/Series/Writer/Artists/Descriptive/File/Catalog).
    /// The C# XmlSerializer writes it on that matcher class; other
    /// matchers never carry it.
    pub option: Option<String>,
}

impl Default for ValueMatcher {
    fn default() -> Self {
        ValueMatcher {
            type_name: String::new(),
            not: false,
            name: String::new(),
            match_value: String::new(),
            match_value_2: String::new(),
            match_operator: 0,
            // C# ComicBookStringMatcher.IgnoreCase default.
            ignore_case: true,
            option: None,
        }
    }
}

/// `ComicBookGroupMatcher`: nested matcher set.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupMatcher {
    /// Inherited from ComicBookMatcher (`Not` attribute).
    pub not: bool,
    pub matcher_mode: MatcherMode,
    pub collapsed: bool,
    pub matchers: Vec<ComicBookMatcher>,
}

impl Default for GroupMatcher {
    fn default() -> Self {
        GroupMatcher {
            not: false,
            matcher_mode: MatcherMode::And,
            collapsed: false,
            matchers: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ComicBookMatcher {
    Value(ValueMatcher),
    Group(GroupMatcher),
}

impl ComicBookMatcher {
    pub fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("ComicBookMatcher")?;
        match self {
            ComicBookMatcher::Value(v) => {
                e.attr("xsi:type", &v.type_name)?;
                if v.not {
                    e.attr("Not", "true")?;
                }
                if !v.name.is_empty() {
                    e.attr("Name", &v.name)?;
                }
                if v.match_operator != 0 {
                    e.attr("MatchOperator", &v.match_operator.to_string())?;
                }
                if !v.ignore_case {
                    e.attr("IgnoreCase", "false")?;
                }
                e.text_elem("MatchValue", &v.match_value)?;
                if !v.match_value_2.is_empty() {
                    e.text_elem("MatchValue2", &v.match_value_2)?;
                }
                if let Some(o) = &v.option {
                    e.text_elem("Option", o)?;
                }
            }
            ComicBookMatcher::Group(g) => {
                e.attr("xsi:type", "ComicBookGroupMatcher")?;
                if g.not {
                    e.attr("Not", "true")?;
                }
                if g.matcher_mode != MatcherMode::And {
                    e.attr("MatcherMode", &g.matcher_mode.to_xml())?;
                }
                if g.collapsed {
                    e.attr("Collapsed", "true")?;
                }
                e.start("Matchers")?;
                for m in &g.matchers {
                    m.write_xml(e)?;
                }
                e.end()?;
            }
        }
        e.end()
    }

    pub fn from_start(s: &Start, r: &mut crate::xml::XmlReader<'_>) -> XmlResult<Self> {
        let xsi = s
            .attr("xsi:type")
            .ok_or_else(|| XmlError("matcher without xsi:type".into()))?;
        if xsi == "ComicBookGroupMatcher" {
            let mut g = GroupMatcher {
                not: false,
                matcher_mode: MatcherMode::And,
                collapsed: false,
                matchers: Vec::new(),
            };
            for (k, v) in &s.attrs {
                match k.as_str() {
                    "Not" => g.not = v.trim() == "true" || v.trim() == "1",
                    "MatcherMode" => {
                        g.matcher_mode = MatcherMode::from_xml(v)
                            .ok_or_else(|| XmlError(format!("bad MatcherMode: {v}")))?
                    }
                    "Collapsed" => g.collapsed = v.trim() == "true" || v.trim() == "1",
                    _ => {}
                }
            }
            loop {
                match r.next_tok()? {
                    Tok::Eof => return Err(XmlError("eof in ComicBookGroupMatcher".into())),
                    Tok::End(n) if n == s.name => {
                        return Ok(ComicBookMatcher::Group(g));
                    }
                    Tok::Start(s2) if s2.name == "Matchers" => loop {
                        match r.next_tok()? {
                            Tok::Eof => return Err(XmlError("eof in Matchers".into())),
                            Tok::End(n) if n == "Matchers" => break,
                            Tok::Start(s3) if s3.name == "ComicBookMatcher" => {
                                g.matchers.push(ComicBookMatcher::from_start(&s3, r)?);
                            }
                            Tok::Start(s3) => r.skip_element(&s3.name)?,
                            Tok::Text(_) => {}
                            _ => {}
                        }
                    },
                    Tok::Start(s2) => r.skip_element(&s2.name)?,
                    Tok::Text(_) => {}
                    _ => {}
                }
            }
        } else {
            let mut v = ValueMatcher {
                type_name: xsi.to_string(),
                not: false,
                name: String::new(),
                match_value: String::new(),
                match_value_2: String::new(),
                match_operator: 0,
                ignore_case: true,
                option: None,
            };
            for (k, val) in &s.attrs {
                match k.as_str() {
                    "Not" => v.not = val.trim() == "true" || val.trim() == "1",
                    "Name" => v.name = val.clone(),
                    "MatchOperator" => {
                        v.match_operator = val
                            .trim()
                            .parse()
                            .map_err(|_| XmlError(format!("bad MatchOperator: {val}")))?
                    }
                    "IgnoreCase" => v.ignore_case = !(val.trim() == "false" || val.trim() == "0"),
                    _ => {}
                }
            }
            loop {
                match r.next_tok()? {
                    Tok::Eof => return Err(XmlError("eof in matcher".into())),
                    Tok::End(n) if n == s.name => return Ok(ComicBookMatcher::Value(v)),
                    Tok::Start(s2) => match s2.name.as_str() {
                        "MatchValue" => v.match_value = r.text_content("MatchValue")?,
                        "MatchValue2" => v.match_value_2 = r.text_content("MatchValue2")?,
                        "Option" => v.option = Some(r.text_content("Option")?),
                        _ => r.skip_element(&s2.name)?,
                    },
                    Tok::Text(_) => {}
                    _ => {}
                }
            }
        }
    }
}

// ---------- List items ----------

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ListItemBase {
    /// IdComponent.Id: always written.
    pub id: CrGuid,
    pub name: Option<String>,
    pub favorite: bool,
    pub book_count: i32,
    pub new_book_count: i32,
    pub new_book_count_date: CrDateTime,
    pub unread_book_count: i32,
    pub description: String,
    pub cache_storage: Option<String>,
    /// Getter is non-null in C#: `<Display>` always written.
    pub display: Option<DisplayListConfig>,
    pub quick_open: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SmartListItem {
    pub base: ListItemBase,
    pub matcher_mode: MatcherMode,
    pub matchers: Vec<ComicBookMatcher>,
    pub limit: bool,
    pub limit_type: ComicSmartListLimitType,
    pub limit_value: i32,
    pub limit_selection_type: ComicSmartListLimitSelectionType,
    pub limit_random_seed: i32,
    pub base_list_id: CrGuid,
    pub not_in_base_list: bool,
    pub filtered_ids: Vec<CrGuid>,
    pub show_filtered: bool,
}

impl Default for SmartListItem {
    fn default() -> Self {
        SmartListItem {
            base: ListItemBase::default(),
            matcher_mode: MatcherMode::And,
            matchers: Vec::new(),
            limit: false,
            limit_type: ComicSmartListLimitType::Count,
            limit_value: 25,
            limit_selection_type: ComicSmartListLimitSelectionType::Random,
            limit_random_seed: 0,
            base_list_id: CrGuid::EMPTY,
            not_in_base_list: false,
            filtered_ids: Vec::new(),
            show_filtered: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FolderItem {
    pub base: ListItemBase,
    pub collapsed: bool,
    pub temporary: bool,
    pub combine_mode: ComicFolderCombineMode,
    pub items: Vec<ComicListItem>,
}

impl Default for FolderItem {
    fn default() -> Self {
        FolderItem {
            base: ListItemBase::default(),
            collapsed: false,
            temporary: false,
            combine_mode: ComicFolderCombineMode::Or,
            items: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct IdListItem {
    pub base: ListItemBase,
    pub book_ids: Vec<CrGuid>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct LibraryListItem {
    pub base: ListItemBase,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ComicListItem {
    Smart(SmartListItem),
    Folder(FolderItem),
    IdList(IdListItem),
    Library(LibraryListItem),
}

impl ComicListItem {
    pub fn base(&self) -> &ListItemBase {
        match self {
            ComicListItem::Smart(i) => &i.base,
            ComicListItem::Folder(i) => &i.base,
            ComicListItem::IdList(i) => &i.base,
            ComicListItem::Library(i) => &i.base,
        }
    }

    pub fn base_mut(&mut self) -> &mut ListItemBase {
        match self {
            ComicListItem::Smart(i) => &mut i.base,
            ComicListItem::Folder(i) => &mut i.base,
            ComicListItem::IdList(i) => &mut i.base,
            ComicListItem::Library(i) => &mut i.base,
        }
    }

    fn xsi_type(&self) -> &'static str {
        match self {
            ComicListItem::Smart(_) => "ComicSmartListItem",
            ComicListItem::Folder(_) => "ComicListItemFolder",
            ComicListItem::IdList(_) => "ComicIdListItem",
            ComicListItem::Library(_) => "ComicLibraryListItem",
        }
    }

    /// Writes `<Item xsi:type="...">`. Attr order: Id, Name, Favorite,
    /// QuickOpen, then concrete attrs. Elements: base first.
    pub fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("Item")?;
        e.attr("xsi:type", self.xsi_type())?;
        self.write_base_attrs(e)?;
        match self {
            ComicListItem::Smart(i) => {
                if i.matcher_mode != MatcherMode::And {
                    e.attr("MatcherMode", &i.matcher_mode.to_xml())?;
                }
            }
            ComicListItem::Folder(i) => {
                if i.collapsed {
                    e.attr("Collapsed", "true")?;
                }
                if i.temporary {
                    e.attr("Temporary", "true")?;
                }
                if i.combine_mode != ComicFolderCombineMode::Or {
                    e.attr("CombineMode", &i.combine_mode.to_xml())?;
                }
            }
            _ => {}
        }
        self.write_base_elements(e)?;
        match self {
            ComicListItem::Smart(i) => {
                e.start("Matchers")?;
                for m in &i.matchers {
                    m.write_xml(e)?;
                }
                e.end()?;
                if i.limit {
                    e.text_elem("Limit", "true")?;
                }
                if i.limit_type != ComicSmartListLimitType::Count {
                    e.text_elem("LimitType", &i.limit_type.to_xml())?;
                }
                if i.limit_value != 25 {
                    e.text_elem("LimitValue", &i.limit_value.to_string())?;
                }
                if i.limit_selection_type != ComicSmartListLimitSelectionType::Random {
                    e.text_elem("LimitSelectionType", &i.limit_selection_type.to_xml())?;
                }
                if i.limit_random_seed != 0 {
                    e.text_elem("LimitRandomSeed", &i.limit_random_seed.to_string())?;
                }
                if !i.base_list_id.is_empty() {
                    e.text_elem("BaseListId", &i.base_list_id.to_d_string())?;
                }
                if i.not_in_base_list {
                    e.text_elem("NotInBaseList", "true")?;
                }
                if !i.filtered_ids.is_empty() {
                    e.start("FilteredIds")?;
                    for g in &i.filtered_ids {
                        e.text_elem("guid", &g.to_d_string())?;
                    }
                    e.end()?;
                }
                if i.show_filtered {
                    e.text_elem("ShowFiltered", "true")?;
                }
            }
            ComicListItem::Folder(i) => {
                e.start("Items")?;
                for item in &i.items {
                    item.write_xml(e)?;
                }
                e.end()?;
            }
            ComicListItem::IdList(i) => {
                e.start("BookIds")?;
                for g in &i.book_ids {
                    e.text_elem("guid", &g.to_d_string())?;
                }
                e.end()?;
            }
            ComicListItem::Library(_) => {}
        }
        e.end()
    }

    fn write_base_attrs<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        let b = self.base();
        e.attr("Id", &b.id.to_d_string())?;
        if let Some(n) = &b.name {
            e.attr("Name", n)?;
        }
        if b.favorite {
            e.attr("Favorite", "true")?;
        }
        if b.quick_open {
            e.attr("QuickOpen", "true")?;
        }
        Ok(())
    }

    fn write_base_elements<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        let b = self.base();
        if b.book_count != 0 {
            e.text_elem("BookCount", &b.book_count.to_string())?;
        }
        if b.new_book_count != 0 {
            e.text_elem("NewBookCount", &b.new_book_count.to_string())?;
        }
        if !b.new_book_count_date.is_min_value() {
            e.text_elem("NewBookCountDate", &b.new_book_count_date.to_xml())?;
        }
        if b.unread_book_count != 0 {
            e.text_elem("UnreadBookCount", &b.unread_book_count.to_string())?;
        }
        if !b.description.is_empty() {
            e.text_elem("Description", &b.description)?;
        }
        if let Some(c) = &b.cache_storage {
            e.text_elem("CacheStorage", c)?;
        }
        // DisplayListConfig::write_xml emits the `<Display>` element
        // itself; the C# field is never null so the wrapper is always
        // written (empty config → `<Display />`).
        match &b.display {
            Some(d) => d.write_xml(e)?,
            None => {
                DisplayListConfig::default().write_xml(e)?;
            }
        }
        Ok(())
    }

    pub fn from_start(s: &Start, r: &mut crate::xml::XmlReader<'_>) -> XmlResult<Self> {
        let xsi = s
            .attr("xsi:type")
            .ok_or_else(|| XmlError("list item without xsi:type".into()))?
            .to_string();
        let mut base = ListItemBase::default();
        let mut concrete: ConcreteItem = match xsi.as_str() {
            "ComicSmartListItem" => ConcreteItem::Smart(SmartListItem::default()),
            "ComicListItemFolder" => ConcreteItem::Folder(FolderItem::default()),
            "ComicIdListItem" => ConcreteItem::IdList(IdListItem::default()),
            "ComicLibraryListItem" => ConcreteItem::Library(LibraryListItem::default()),
            other => return Err(XmlError(format!("unknown list item type: {other}"))),
        };
        for (k, v) in &s.attrs {
            let flag = || v.trim() == "true" || v.trim() == "1";
            match k.as_str() {
                "Id" => base.id = CrGuid::parse(v)?,
                "Name" => base.name = Some(v.clone()),
                "Favorite" => base.favorite = flag(),
                "QuickOpen" => base.quick_open = flag(),
                "MatcherMode" => {
                    if let ConcreteItem::Smart(ref mut i) = concrete {
                        i.matcher_mode = MatcherMode::from_xml(v)
                            .ok_or_else(|| XmlError(format!("bad MatcherMode: {v}")))?;
                    }
                }
                "Collapsed" => {
                    if let ConcreteItem::Folder(ref mut i) = concrete {
                        i.collapsed = flag();
                    }
                }
                "Temporary" => {
                    if let ConcreteItem::Folder(ref mut i) = concrete {
                        i.temporary = flag();
                    }
                }
                "CombineMode" => {
                    if let ConcreteItem::Folder(ref mut i) = concrete {
                        i.combine_mode = ComicFolderCombineMode::from_xml(v)
                            .ok_or_else(|| XmlError(format!("bad CombineMode: {v}")))?;
                    }
                }
                _ => {}
            }
        }
        // Now walk children.
        loop {
            match r.next_tok()? {
                Tok::Eof => return Err(XmlError("eof in list item".into())),
                Tok::End(n) if n == s.name => {
                    let item = match concrete {
                        ConcreteItem::Smart(mut i) => {
                            i.base = base;
                            ComicListItem::Smart(i)
                        }
                        ConcreteItem::Folder(mut i) => {
                            i.base = base;
                            ComicListItem::Folder(i)
                        }
                        ConcreteItem::IdList(mut i) => {
                            i.base = base;
                            ComicListItem::IdList(i)
                        }
                        ConcreteItem::Library(mut i) => {
                            i.base = base;
                            ComicListItem::Library(i)
                        }
                    };
                    return Ok(item);
                }
                Tok::Start(s2) => match s2.name.as_str() {
                    "Display" => {
                        // Full parser consumes through `</Display>`.
                        base.display = Some(DisplayListConfig::from_start(&s2, r)?);
                    }
                    "Matchers" if matches!(concrete, ConcreteItem::Smart(_)) => {
                        let ConcreteItem::Smart(ref mut i) = concrete else {
                            unreachable!()
                        };
                        loop {
                            match r.next_tok()? {
                                Tok::Eof => return Err(XmlError("eof in Matchers".into())),
                                Tok::End(n) if n == "Matchers" => break,
                                Tok::Start(s3) if s3.name == "ComicBookMatcher" => {
                                    i.matchers.push(ComicBookMatcher::from_start(&s3, r)?);
                                }
                                Tok::Start(s3) => r.skip_element(&s3.name)?,
                                Tok::Text(_) => {}
                                _ => {}
                            }
                        }
                    }
                    "Items" if matches!(concrete, ConcreteItem::Folder(_)) => {
                        let ConcreteItem::Folder(ref mut i) = concrete else {
                            unreachable!()
                        };
                        loop {
                            match r.next_tok()? {
                                Tok::Eof => return Err(XmlError("eof in Items".into())),
                                Tok::End(n) if n == "Items" => break,
                                Tok::Start(s3) if s3.name == "Item" => {
                                    i.items.push(ComicListItem::from_start(&s3, r)?);
                                }
                                Tok::Start(s3) => r.skip_element(&s3.name)?,
                                Tok::Text(_) => {}
                                _ => {}
                            }
                        }
                    }
                    "BookIds" if matches!(concrete, ConcreteItem::IdList(_)) => {
                        let ConcreteItem::IdList(ref mut i) = concrete else {
                            unreachable!()
                        };
                        loop {
                            match r.next_tok()? {
                                Tok::Eof => return Err(XmlError("eof in BookIds".into())),
                                Tok::End(n) if n == "BookIds" => break,
                                Tok::Start(s3) => {
                                    let v = r.text_content(&s3.name)?;
                                    i.book_ids.push(CrGuid::parse(v.trim())?);
                                }
                                Tok::Text(_) => {}
                                _ => {}
                            }
                        }
                    }
                    _ => {
                        read_base_or_concrete_element(&s2, r, &mut base, &mut concrete)?;
                    }
                },
                Tok::Text(_) => {}
                _ => {}
            }
        }
    }
}

enum ConcreteItem {
    Smart(SmartListItem),
    Folder(FolderItem),
    IdList(IdListItem),
    Library(LibraryListItem),
}

fn read_base_or_concrete_element(
    s2: &Start,
    r: &mut crate::xml::XmlReader<'_>,
    base: &mut ListItemBase,
    concrete: &mut ConcreteItem,
) -> XmlResult<()> {
    match s2.name.as_str() {
        "BookCount" => base.book_count = read_i32(r)?,
        "NewBookCount" => base.new_book_count = read_i32(r)?,
        "NewBookCountDate" => {
            let v = r.text_content("NewBookCountDate")?;
            base.new_book_count_date = CrDateTime::parse(v.trim()).map_err(|e| XmlError(e.0))?;
        }
        "UnreadBookCount" => base.unread_book_count = read_i32(r)?,
        "Description" => base.description = r.text_content("Description")?,
        "CacheStorage" => base.cache_storage = Some(r.text_content("CacheStorage")?),
        "Limit" => {
            let ConcreteItem::Smart(ref mut i) = *concrete else {
                r.skip_element("Limit")?;
                return Ok(());
            };
            i.limit = read_bool(r)?;
        }
        "LimitType" => {
            let ConcreteItem::Smart(ref mut i) = *concrete else {
                r.skip_element("LimitType")?;
                return Ok(());
            };
            let v = r.text_content("LimitType")?;
            i.limit_type = ComicSmartListLimitType::from_xml(v.trim())
                .ok_or_else(|| XmlError(format!("bad LimitType: {v}")))?;
        }
        "LimitValue" => {
            let ConcreteItem::Smart(ref mut i) = *concrete else {
                r.skip_element("LimitValue")?;
                return Ok(());
            };
            i.limit_value = read_i32(r)?;
        }
        "LimitSelectionType" => {
            let ConcreteItem::Smart(ref mut i) = *concrete else {
                r.skip_element("LimitSelectionType")?;
                return Ok(());
            };
            let v = r.text_content("LimitSelectionType")?;
            i.limit_selection_type = ComicSmartListLimitSelectionType::from_xml(v.trim())
                .ok_or_else(|| XmlError(format!("bad LimitSelectionType: {v}")))?;
        }
        "LimitRandomSeed" => {
            let ConcreteItem::Smart(ref mut i) = *concrete else {
                r.skip_element("LimitRandomSeed")?;
                return Ok(());
            };
            i.limit_random_seed = read_i32(r)?;
        }
        "BaseListId" => {
            let ConcreteItem::Smart(ref mut i) = *concrete else {
                r.skip_element("BaseListId")?;
                return Ok(());
            };
            let v = r.text_content("BaseListId")?;
            i.base_list_id = CrGuid::parse(v.trim())?;
        }
        "NotInBaseList" => {
            let ConcreteItem::Smart(ref mut i) = *concrete else {
                r.skip_element("NotInBaseList")?;
                return Ok(());
            };
            i.not_in_base_list = read_bool(r)?;
        }
        "FilteredIds" => {
            let ConcreteItem::Smart(ref mut i) = *concrete else {
                r.skip_element("FilteredIds")?;
                return Ok(());
            };
            loop {
                match r.next_tok()? {
                    Tok::Eof => return Err(XmlError("eof in FilteredIds".into())),
                    Tok::End(n) if n == "FilteredIds" => break,
                    Tok::Start(s3) => {
                        let v = r.text_content(&s3.name)?;
                        i.filtered_ids.push(CrGuid::parse(v.trim())?);
                    }
                    Tok::Text(_) => {}
                    _ => {}
                }
            }
        }
        "ShowFiltered" => {
            let ConcreteItem::Smart(ref mut i) = *concrete else {
                r.skip_element("ShowFiltered")?;
                return Ok(());
            };
            i.show_filtered = read_bool(r)?;
        }
        _ => r.skip_element(&s2.name)?,
    }
    Ok(())
}

fn read_i32(r: &mut crate::xml::XmlReader<'_>) -> XmlResult<i32> {
    // The element name was consumed by the caller's Start; text until End.
    match r.next_tok()? {
        Tok::Text(t) => {
            expect_end(r)?;
            t.trim()
                .parse()
                .map_err(|_| XmlError(format!("bad int: {t}")))
        }
        Tok::End(_) => Ok(0),
        Tok::Eof => Err(XmlError("eof expecting int".into())),
        Tok::Start(_) => Err(XmlError("unexpected nested element in int".into())),
    }
}

fn read_bool(r: &mut crate::xml::XmlReader<'_>) -> XmlResult<bool> {
    match r.next_tok()? {
        Tok::Text(t) => {
            expect_end(r)?;
            Ok(t.trim() == "true" || t.trim() == "1")
        }
        Tok::End(_) => Ok(false),
        Tok::Eof => Err(XmlError("eof expecting bool".into())),
        Tok::Start(_) => Err(XmlError("unexpected nested element in bool".into())),
    }
}

fn expect_end(r: &mut crate::xml::XmlReader<'_>) -> XmlResult<()> {
    match r.next_tok()? {
        Tok::End(_) => Ok(()),
        _ => Err(XmlError("expected end tag".into())),
    }
}

// ---------- WatchFolder ----------

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct WatchFolder {
    /// No DefaultValue: always written (empty string too).
    pub folder: String,
    pub watch: bool,
}

impl WatchFolder {
    pub fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("WatchFolder")?;
        e.attr("Folder", &self.folder)?;
        if self.watch {
            e.attr("Watch", "true")?;
        }
        e.end()
    }
}
