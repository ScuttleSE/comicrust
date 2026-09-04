//! Enum types in the XML surface, with the exact serialized names of the
//! C# enum members. Parse is case-sensitive like `Enum.TryParse(false)`;
//! numeric strings are accepted like the .NET converter.

/// Generates a plain (non-flags) XML enum: named members, numeric
/// fallback, case-sensitive parse. `default` names the C# field default.
macro_rules! xml_enum {
    ($(#[$meta:meta])* $name:ident : $repr:ty { $( $variant:ident = $value:expr ),+ $(,)? } default = $def:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $( $variant ),+
        }

        impl Default for $name {
            fn default() -> Self {
                $name::$def
            }
        }

        impl $name {
            #[allow(dead_code)]
            pub const VALUES: &'static [(&'static str, $repr)] = &[ $( (stringify!($variant), $value as $repr) ),+ ];

            pub fn from_repr(v: $repr) -> Option<Self> {
                #[allow(unreachable_patterns)] // C# aliases (Default = x, BestQuality = y)
                match v {
                    $( $value => Some($name::$variant), )+
                    _ => None,
                }
            }

            #[allow(dead_code)]
            pub fn repr(self) -> $repr {
                match self {
                    $( $name::$variant => $value as $repr, )+
                }
            }

            /// XML string: member name; unknown numbers fall back to the
            /// decimal form (mirrors .NET `ToString`).
            pub fn to_xml(self) -> String {
                for (n, v) in Self::VALUES {
                    if *v == self.repr() {
                        return (*n).to_string();
                    }
                }
                self.repr().to_string()
            }

            /// Case-sensitive name parse; numeric strings accepted;
            /// anything else fails like the .NET serializer.
            pub fn from_xml(s: &str) -> Option<Self> {
                for (n, v) in Self::VALUES {
                    if *n == s {
                        return Self::from_repr(*v);
                    }
                }
                s.trim().parse::<$repr>().ok().and_then(Self::from_repr)
            }
        }

        impl $crate::settings::registry::EnumValue for $name {
            fn from_int(v: i32) -> Option<Self> {
                <$repr as TryFrom<i32>>::try_from(v).ok().and_then(Self::from_repr)
            }

            fn from_name(s: &str) -> Option<Self> {
                Self::from_xml(s).or_else(|| {
                    Self::VALUES
                        .iter()
                        .find(|(n, _)| n.eq_ignore_ascii_case(s))
                        .and_then(|(_, v)| Self::from_repr(*v))
                })
            }

            fn as_name(&self) -> String {
                Self::to_xml(*self)
            }
        }
    };
}

/// Same as [`xml_enum`] but for `[Flags]` enums: composite members match
/// by name first, otherwise the value decomposes into named bits in
/// ascending order, joined with `", "`.
macro_rules! xml_flags {
    ($(#[$meta:meta])* $name:ident : $repr:ty { $( $variant:ident = $value:expr ),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        pub struct $name(pub $repr);

        impl $name {
            #[allow(dead_code)]
            pub const MEMBERS: &'static [(&'static str, $repr)] = &[ $( (stringify!($variant), $value as $repr) ),+ ];

            pub fn to_xml(self) -> String {
                if let Some((n, _)) = Self::MEMBERS.iter().find(|(_, v)| *v == self.0) {
                    return (*n).to_string();
                }
                if self.0 == 0 {
                    return "None".to_string();
                }
                let mut rest = self.0;
                let mut parts: Vec<&str> = Vec::new();
                for (n, v) in Self::MEMBERS {
                    if *v != 0 && rest & *v == *v {
                        // only single-bit members take part in the list
                        if v.count_ones() == 1 {
                            parts.push(n);
                            rest &= !*v;
                        }
                    }
                }
                if rest == 0 && !parts.is_empty() {
                    parts.join(", ")
                } else {
                    self.0.to_string()
                }
            }

            /// Case-sensitive; accepts member names, flag lists
            /// (`"FrontCover, BackCover"`) and numeric strings. A partial
            /// match that consumes nothing fails (like TryParse).
            pub fn from_xml(s: &str) -> Option<Self> {
                if let Some((_, v)) = Self::MEMBERS.iter().find(|(n, _)| *n == s) {
                    return Some($name(*v));
                }
                if s.trim().is_empty() {
                    return None;
                }
                if let Ok(v) = s.trim().parse::<$repr>() {
                    return Some($name(v));
                }
                let mut acc: $repr = 0;
                for part in s.split(',') {
                    let part = part.trim();
                    if part.is_empty() {
                        return None;
                    }
                    match Self::MEMBERS.iter().find(|(n, _)| *n == part) {
                        Some((_, v)) => acc |= *v,
                        None => match part.parse::<$repr>() {
                            Ok(v) => acc |= v,
                            Err(_) => return None,
                        },
                    }
                }
                Some($name(acc))
            }
        }

        impl $crate::settings::registry::EnumValue for $name {
            fn from_int(v: i32) -> Option<Self> {
                <$repr as TryFrom<i32>>::try_from(v).ok().map($name)
            }

            fn from_name(s: &str) -> Option<Self> {
                Self::from_xml(s).or_else(|| {
                    Self::MEMBERS
                        .iter()
                        .find(|(n, _)| n.eq_ignore_ascii_case(s))
                        .map(|(_, v)| $name(*v))
                })
            }

            fn as_name(&self) -> String {
                Self::to_xml(*self)
            }
        }
    };
}

xml_enum! {
   /// `cYo.Common.ComponentModel`? No: `cYo.Projects.ComicRack.Engine` YesNo.
   YesNo : i32 { Unknown = -1, No = 0, Yes = 1 }
default = Unknown}

xml_enum! {
   MangaYesNo : i32 { Unknown = -1, No = 0, Yes = 1, YesAndRightToLeft = 2 }
default = Unknown}

xml_flags! {
    /// `ComicPageType` (short, flags).
    ComicPageType : i32 {
        FrontCover = 1, InnerCover = 2, Roundup = 4, Story = 8,
        Advertisement = 16, Editorial = 32, Letters = 64, Preview = 128,
        BackCover = 256, Other = 512, Deleted = 1024,
        All = 1023, AllWithDeleted = 2047
    }
}

xml_enum! {
   ImageRotation : u8 { None = 0, Rotate90 = 1, Rotate180 = 2, Rotate270 = 3 }
default = None}

xml_enum! {
   ComicPagePosition : i32 { Default = 0, Near = 1, Far = 2 }
default = Default}

xml_enum! {
   MatcherMode : i32 { And = 0, Or = 1 }
default = And}

xml_enum! {
   ComicFolderCombineMode : i32 { Or = 0, And = 1, Empty = 2 }
default = Or}

xml_enum! {
   ComicSmartListLimitType : i32 { Count = 0, MB = 1, GB = 2 }
default = Count}

xml_enum! {
   ComicSmartListLimitSelectionType : i32 { Position = 0, SortedBySeries = 1, Random = 2 }
default = Random}

xml_enum! {
   ItemViewMode : i32 { Thumbnail = 0, Tile = 1, Detail = 2 }
default = Detail}

xml_enum! {
   /// `System.Windows.Forms.SortOrder`.
   SortOrder : i32 { Ascending = 0, Descending = 1, None = 2 }
default = Ascending}

xml_enum! {
   GroupStatus : i32 { AllExpanded = 0, AllCollapsed = 1, KeysExpanded = 2, KeysCollapsed = 3 }
default = AllExpanded}

xml_enum! {
   ShowOptionType : i32 { All = 0, Read = 1, Reading = 2, Unread = 3 }
default = All}

xml_enum! {
   ShowComicType : i32 { All = 0, Comics = 1, FilelessComics = 2 }
default = All}

xml_enum! {
   MatcherOption : i32 { All = 0, Series = 1, Writer = 2, Artists = 3, Descriptive = 4, File = 5, Catalog = 6 }
default = All}

xml_flags! {
    BitmapAdjustmentOptions : i32 { None = 0, AutoContrast = 1 }
}

xml_flags! {
    /// `ComicTextElements` — long list with named composite members that
    /// serialize as single names (`DefaultFileComic`).
    ComicTextElements : i32 {
        None = 0x0, Caption = 0x1, CaptionWithoutTitle = 0x2, AlternateCaption = 0x4,
        Title = 0x8, ArtistInfo = 0x10, Summary = 0x20, FileSize = 0x40,
        Opened = 0x80, FileName = 0x100, FileFormat = 0x200, Added = 0x400,
        PublisherAndImprint = 0x800, AgeRating = 0x1000, Genre = 0x2000,
        CharactersAndTeams = 0x4000, Locations = 0x8000, Notes = 0x10000,
        PurchaseInformation = 0x20000, StorageInfoformation = 0x40000,
        CollectionStatus = 0x80000, ScanInformation = 0x100000, Released = 0x200000,
        StackTitle = 0x1000000, StackBookCount = 0x2000000, StackBooksOpened = 0x4000000,
        NoEmptyDates = 0x10000000, DefaultComic = 0x27A, DefaultFileComic = 0x37A,
        AllComic = 0x3FFFFF, DefaultStack = 0x7000050, LinkedElements = 0x340,
        DefaultPage = 0x8000000
    }
}

pub(crate) use xml_enum;
pub(crate) use xml_flags;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_type_names() {
        assert_eq!(ComicPageType(8).to_xml(), "Story");
        assert_eq!(ComicPageType(1).to_xml(), "FrontCover");
        assert_eq!(ComicPageType(1 | 256).to_xml(), "FrontCover, BackCover");
        assert_eq!(ComicPageType(1023).to_xml(), "All");
        assert_eq!(ComicPageType::from_xml("Advertisement").unwrap().0, 16);
        // legacy misspelling is fixed by the CALLER (ComicPageInfo), not here
        assert_eq!(ComicPageType::from_xml("Advertisment"), None);
        assert_eq!(
            ComicPageType::from_xml("FrontCover, BackCover").unwrap().0,
            257
        );
        assert_eq!(ComicPageType::from_xml("16").unwrap().0, 16);
    }

    #[test]
    fn text_elements_composites() {
        assert_eq!(ComicTextElements(0x37A).to_xml(), "DefaultFileComic");
        assert_eq!(
            ComicTextElements::from_xml("DefaultFileComic").unwrap().0,
            0x37A
        );
        assert_eq!(ComicTextElements(0x27A).to_xml(), "DefaultComic");
        assert_eq!(ComicTextElements(0x340).to_xml(), "LinkedElements");
    }

    #[test]
    fn yes_no() {
        assert_eq!(YesNo::Unknown.to_xml(), "Unknown");
        assert_eq!(YesNo::from_xml("Yes"), Some(YesNo::Yes));
        assert_eq!(YesNo::from_xml("yes"), None);
        assert_eq!(MangaYesNo::YesAndRightToLeft.to_xml(), "YesAndRightToLeft");
        assert_eq!(YesNo::from_xml("-1"), Some(YesNo::Unknown));
        assert_eq!(YesNo::from_xml("bogus"), None);
    }
}
