//! Settings-scoped C# enums (`ComicRack.Engine/Display/*.cs`,
//! `ComicRack/Config/*.cs`), with the exact serialized member names.

use crate::model::enums::{xml_enum, xml_flags};

xml_enum!(Themes: i32 { Default = 0, Dark = 1 } default = Default);

xml_enum!(MagnifierStyle: i32 { Glass = 0, Simple = 1 } default = Glass);

xml_enum!(RightToLeftReadingMode: i32 { FlipParts = 0, FlipPages = 1 } default = FlipPages);

xml_flags!(ImageDisplayOptions: i32 {
    None = 0x0,
    HighQuality = 0x1,
    AnamorphicScaling = 0x2,
    Default = 0x1,
});

xml_flags!(LibraryGauges: i32 {
    None = 0x0,
    New = 0x1,
    Unread = 0x2,
    Total = 0x4,
    Numeric = 0x1000,
    Default = 0x1007,
});

xml_flags!(TabLayouts: i32 {
    None = 0x0,
    Paste = 0x1,
    Export = 0x2,
    Multiple = 0x4,
    ReaderSettings = 0x8,
    BehaviorSettings = 0x10,
    LibrarySettings = 0x20,
    ScriptSettings = 0x40,
    AdvancedSettings = 0x80,
});

xml_flags!(HiddenMessageBoxes: i32 {
    None = 0x0,
    RemoveFromList = 0x1,
    RemoveList = 0x2,
    RemoveFavorite = 0x4,
    ConvertComics = 0x8,
    SetAllListLayouts = 0x10,
    CloseExternalReader = 0x20,
    ComicRackMinimized = 0x40,
    AskDirtyItems = 0x80,
    AskClearData = 0x100,
    DoNotCheckForUpdate = 0x200,
    NeverAskDirtyItems = 0x400,
});

xml_enum!(QueryCacheMode: i32 {
    Disabled = 0,
    InstantUpdate = 1,
    DelayedUpdate = 2,
} default = Disabled);

xml_enum!(CbEngine: i32 {
    SevenZip = 0,
    SevenZipExe = 1,
    SharpCompress = 2,
    SharpZip = 3,
} default = SevenZip);

xml_enum!(PdfEngine: i32 {
    Ghostscript = 0,
    Pdfium = 1,
    Native = 2,
} default = Ghostscript);

xml_enum!(BitmapResampling: i32 {
    FastAndUgly = 0,
    FastBilinear = 1,
    FastBicubic = 2,
    BilinearHQ = 3,
    GdiPlus = 4,
    GdiPlusHQ = 5,
    Default = 3,
    BestQuality = 5,
} default = FastAndUgly);
