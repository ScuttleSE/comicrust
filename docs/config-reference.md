# comicrust configuration reference

comicrust keeps ONE configuration file:

```
~/.config/comicrust/comicrust.toml
```

Set `XDG_CONFIG_HOME` to move the location. The Comic Vine Scraper
history cache (`prior_series.json`) is a separate file under
`plugins/comic-vine-scraper/`. The database (`ComicDb.xml`) is NOT
part of this file.

## How the file works

- The app reads the file once at boot. Hand edits apply at the next start.
- The app rewrites the whole file at every save point. Values survive. Comments do not.
- Every changeable key appears in the file at its default. A key line you delete comes back at the default on the next boot.
- Precedence: built-in default, then this file, then command line switches.
- A missing or corrupt file is rebuilt from defaults.
- Some keys describe Windows-only or not yet ported features. Such a key has no effect. It is carried for ComicRack parity.

## Editing rules

- Edit one value at a time. Keep the `KEY = value` shape.
- Delete a key line to return that key to the built-in default.
- String values take quotes. Numbers and booleans do not.
- The app accepts the C# ini spellings of the values (`Dark`, `SevenZip`).

## `version`

| Key | Default | Meaning |
|---|---|---|
| `version` | 1 | Config schema version. Do not edit. |

## `[extended]`

Application switches (the old `ComicRack.ini` keys).

| Key | Default | Meaning |
|---|---|---|
| `AllowCopyListFolders` | false | Copy/Paste List can duplicate smart list folders. Same as `-aclf`. |
| `AnamorphicScalingTolerance` | 0.25 | How much width and height scaling may differ from the exact fit (fraction). |
| `AutoHideCursorDuration` | 5000 | Milliseconds before the reader hides the mouse cursor in full screen. |
| `CachePath` | "" | Cache root override. Empty = the default cache location. Same as `-cp`. |
| `ComicCountAlpha` | 64 | Alpha (0-255) of the comic count text in the browser. |
| `ConsolidateDatabase` | false | C# switch parity. No effect in this port. |
| `DataSource` | "" | Remote database connection string (C# `mysql:`/`mssql:` forms). No effect in this port. |
| `DatabaseBackgroundSaving` | 600 | Seconds between background saves of the database. The save also runs during scans. Minimum 1. |
| `DatabasePath` | "" | Database folder override. Empty = the default data folder. Same as `-db`. |
| `DisableAutoTuneSystem` | false | C# performance autotune gate. No effect in this port. |
| `DisableBackgroundQueryCacheUpdate` | true | Stops the background refresh of the query cache. |
| `DisableBroadcast` | false | C# local network broadcaster gate. No effect in this port. |
| `DisableFoldersView` | false | Hides the Folders view tab. |
| `DisableHardware` | false | C# OpenGL gate. No effect in this port. The reader renders through GTK. |
| `DisableListSpinButtons` | false | C# list spin buttons gate. |
| `DisableMenuHideShowAnimation` | false | C# menu animation gate. No effect in this port. |
| `DisableMipMapping` | false | C# OpenGL texture option. No effect in this port. |
| `DisableScriptOptimization` | false | C# IronPython option. No effect in this port. |
| `DoNotLoadQueryCaches` | false | Skips loading saved query caches at boot. |
| `DoNotResetZoomOnBookOpen` | false | Keeps the current zoom when a book opens. |
| `DragDropCursorAlpha` | 0.6 | Alpha of the drag-and-drop preview image. |
| `EnableGroupNameCompression` | false | Compresses group captions to short forms. |
| `ForceHardware` | false | C# OpenGL gate. No effect in this port. |
| `ForceTanColorSchema` | false | C# WinForms toolstrip color scheme. No effect in this port. |
| `HideBrowserIfShellOpen` | true | Hides the browser when an external open hands a file to the app. Set false to keep the browser. |
| `InternetServerPort` | 7612 | Port for internet shares (C# remote server feature). |
| `KeyboardZoomStepping` | 0.5 | Keyboard zoom step as a fraction. 0.5 = 50 percent. |
| `LegacyStackSorting` | false | Uses the older rule to find the first issue of a stack. |
| `LimitMemory` | 0 | C# process memory cap in MB. 0 = off. No effect in this port. |
| `ListMenuSize` | 25 | Row count of the popup menus that show library lists. |
| `LoadDatabaseInForeground` | false | C# boot gate. This port always loads the database at boot. |
| `MacCompatibleScanning` | true | Skips macOS junk files during scans. |
| `MouseSwitchesToFullLibrary` | false | A single click returns from the reader to the full browser. |
| `OnlyLocalRemoteLibrariesInQuickOpen` | true | Quick Open lists only local remote libraries (C# remote feature). |
| `OpenExplorerUsingAPI` | true | C# file manager open API. This port uses `xdg-open`. |
| `OptimizedListScrolling` | false | C# reduced quality scrolling gate. No effect in this port. |
| `PrivateServerPort` | 7612 | Port for private shares (C# remote server feature). |
| `QueryCacheMode` | "InstantUpdate" | Query cache mode. Valid: `Disabled`, `InstantUpdate`, `DelayedUpdate`. |
| `QuickOpenListSize` | 10 | Book count of the Quick Open lists. |
| `RegisterFormats` | "" | C# file association registration list. No effect in this port. |
| `RemoteLibrariesInQuickOpen` | true | Quick Open shows remote libraries (C# remote feature). |
| `ReplaceDefaultListsInQuickOpen` | false | Own quick lists replace the built-in ones. |
| `ScanFileTimeoutSeconds` | 120 | Port addition. The longest time a library scan may spend on ONE file. When the time runs out, the scan marks the book "Timed out" and continues with the next file. 0 removes the limit. |
| `ScanRetryFailedFiles` | false | Port addition. Read files again that already carry an unchanged scan failure. The default leaves them alone, so a rescan does not pay for the same failures again. A changed file is always read again. |
| `ShowContextHelpKey` | false | C# WinForms help gate. No effect in this port. |
| `ShowCustomScriptValues` | false | Shows dotted custom values (script values) in Properties. |
| `ShowScriptConsole` | false | C# IronPython console. No effect in this port. |
| `SortNetworkFolders` | true | Sorts entries in the Folders view. |
| `StartHidden` | false | C# start minimized gate. No effect in this port. |
| `SystemToolBars` | false | C# WinForms toolstrip rendering. No effect in this port. |
| `Theme` | "Default" | UI theme. Valid: `Default`, `Dark`. |
| `UseDarkMode` | false | Legacy dark UI switch (C# `-dark`). Prefer `Theme = "Dark"`. |
| `UseLocalSettings` | false | C# portable mode gate (`-local`). This port keeps data under XDG. |

## `[engine]`

Reader, parsing, image, and archive engine settings.

| Key | Default | Meaning |
|---|---|---|
| `AeroFullScreenWorkaround` | true | C# fullscreen driver workaround. No effect in this port. |
| `AnimationDuration` | 250 | Duration of animated transitions in ms. |
| `BlankPageColor` | "White" | Color of the empty slot in a forced two page spread. Named colors or `r, g, b`. |
| `BlendDuration` | 400 | Duration of the blend page transition in ms. |
| `BookmarkColors` | "Orange, Green, Red, Blue" | Colors of the read position pennants on covers. Named colors or `r, g, b`. |
| `CacheThumbnailPages` | false | Caches the full pages that thumbnail creation decoded. |
| `Cb7Uses` | "SevenZip" | Archive engine for CB7. Valid: `SevenZip`, `SevenZipExe`, `SharpCompress`. |
| `CbrUses` | "SevenZip" | Archive engine for CBR. Valid: `SevenZip`, `SevenZipExe`, `SharpCompress`. |
| `CbtUses` | "SevenZip" | Archive engine for CBT. Valid: `SevenZip`, `SevenZipExe`, `SharpCompress`, `SharpZip`. |
| `CbzUses` | "SevenZip" | Archive engine for CBZ. Valid: `SevenZip`, `SevenZipExe`, `SharpZip`, `SharpCompress`. |
| `ComicCaptionFormat` | "[{format} ][{series}][ {volume}][ #{number}][ - {title}][ ({year}[/{month}[/{day}]])]" | Caption format of book display names. |
| `ComicExportFileNameFormat` | "[{format} ][{series}][ {volume}][ #{number}][ ({year}[/{month}])]" | File name format for exported books. |
| `DisableNTFS` | false | Skips metadata read and write in file streams. The port maps the C# NTFS streams to xattrs. |
| `DjVuLibreInstall` | "" | Path override for the DjVuLibre tools. |
| `DjVuSizeLimit` | "2000, 2000" | Maximum rendered size of a DjVu page (`width, height`). |
| `EnableHtmlScriptErrors` | false | C# HTML info panel option. |
| `EnableParallelQueries` | true | C# parallel query optimization gate. |
| `ExportResampling` | "GdiPlusHQ" | Filter for page resizing during export. Valid: `FastAndUgly`, `FastBilinear`, `FastBicubic`, `BilinearHQ`, `GdiPlus`, `GdiPlusHQ`, `BestQuality`. |
| `ExtraWifiDeviceAddresses` | "" | C# device sync addresses. No effect in this port. |
| `ForceJpegReconstruction` | false | C# JPEG XL lossless export option. Causes a quality loss (C# warning). |
| `FreeDeviceMemoryMB` | 128 | C# device sync memory reserve. No effect in this port. |
| `GestureAreaSize` | 80 | C# touch gesture area size. No effect in this port. |
| `GhostscriptExecutable` | "" | Direct path to the Ghostscript executable (PDF engine override). |
| `HideVisiblePartOverlayClose` | false | Hides the close button on the visible part overlay. |
| `HtmlInfoContextMenu` | false | C# HTML info context menu gate. |
| `IgnoreEmbeddedComicBookXml` | false | The scan skips the embedded ComicBook.xml when true. |
| `IgnoredArticles` | "" | Articles ignored when sorting and grouping. Example: `the, der, die, das, le, la, les, l'`. Empty = built-in default. |
| `IsNotReadCompletionPercentage` | 10 | A book below this read percentage counts as Not Read. |
| `IsReadCompletionPercentage` | 95 | A book at or above this read percentage counts as Read. |
| `IsRecentInDays` | 14 | Days a book stays "recent" for the default smart lists. |
| `JpegXLEncoderEffort` | 7 | JPEG XL compression effort (1-9). Higher = smaller files, slower. |
| `LegacyFilenameParser` | false | Uses the old filename parsing rules. |
| `ListCoverAlpha` | 0.3 | Alpha of the background cover image behind lists. |
| `ListCoverSize` | "512, 512" | Size of the generated background cover image (`width, height`). |
| `MaximumQueueThreads` | 4 | Worker thread cap per background queue. |
| `MaximumUpdateThreads` | 2 | Worker thread cap for the metadata update queue. |
| `MirroredPageTurnAnimation` | false | Uses one animation for both page turn directions. |
| `NavigationPanelWidth` | 0.9 | Reader navigation panel width as a fraction of the total width. |
| `OfValues` | "" | "of" words for the filename parser. Example: `of, von, de`. Empty = built-in default. |
| `OperationTimeout` | 300 | Seconds before a server request times out (C# remote feature). |
| `PageBowBorder` | true | Draws a border on the page bow. |
| `PageBowCenter` | true | Centers the page bow. |
| `PageBowColor` | "Black" | Page bow color. Named colors or `r, g, b`. |
| `PageBowFromAlpha` | 92 | Starting alpha of the page bow (0-255). |
| `PageBowToAlpha` | 0 | Ending alpha of the page bow (0-255). |
| `PageBowWidth` | 0.07 | Page bow width as a fraction of the page width (0.01-0.5). |
| `PageCachingDelay` | 1000 | Delay in ms before page caching starts after a page turn. |
| `PageScrollingDuration` | 1000 | Smooth scroll duration in ms per page. |
| `PageShadowOpacity` | 0.6 | Opacity of the page shadow. |
| `PageShadowWidthPercentage` | 1.0 | Page shadow width in percent of the page width. |
| `ParallelConversions` | 32 | Maximum parallel conversions (C# export queue). |
| `PdfEngineToUse` | "Pdfium" | PDF engine. Valid: `Pdfium`, `Native`, `Ghostscript`. Ghostscript must be installed. |
| `PdfiumImageSize` | "1920, 2540" | Maximum rendered size of a PDF page (`width, height`). |
| `RatingStarsBelowThumbnails` | true | Draws rating stars below the thumbnails. |
| `SearchBrowserCaseSensitive` | false | The quick search is case sensitive when true. |
| `ServerProviderCacheSize` | 100 | Cached provider count in the C# server. No effect in this port. |
| `ShowGestureHint` | true | C# gesture hint gate. No effect in this port. |
| `SoftwareFilter` | "GdiPlusHQ" | Filter for the additional sharpening pass. Same values as `ExportResampling`. |
| `SoftwareFilterDelay` | 1000 | Delay in ms before the software filter applies. |
| `SoftwareFilterMinScale` | 0.05 | Minimum scale where the software filter kicks in. |
| `SyncCreateThumbnails` | true | C# device sync option. No effect in this port. |
| `SyncKeepReadComics` | 1 | C# device sync option. No effect in this port. |
| `SyncOptimizeMaxHeight` | 1500 | C# device sync option. No effect in this port. |
| `SyncOptimizeQuality` | 65 | C# device sync option. No effect in this port. |
| `SyncOptimizeSharpen` | false | C# device sync option. No effect in this port. |
| `SyncOptimizeWebP` | true | C# device sync option. No effect in this port. |
| `SyncQueueLength` | 50 | C# device sync option. No effect in this port. |
| `SyncResamping` | "GdiPlus" | C# device sync resampling filter. The C# spells the key `Resamping`. |
| `SyncWebP` | false | C# device sync option. No effect in this port. |
| `TempPath` | "/tmp" | Root for temporary files. Must be an existing directory. |
| `ThumbnailPageBow` | true | Draws the page bow on thumbnails. |
| `ThumbnailPageCurlColor` | 0 | Page curl color on thumbnails (RGB as int). |
| `ThumbnailQuality` | 60 | JPEG quality of cached thumbnails (0-100). |
| `ThumbnailResampling` | "FastBilinear" | Filter for thumbnail creation. Same values as `ExportResampling`. |
| `UseLegacyZipConfiguration` | false | Uses the older CBZ entry settings (Deflate instead of Store, no NTFS timestamps). |
| `WifiSyncConnectionRetries` | 1 | C# device sync option. No effect in this port. |
| `WifiSyncConnectionTimeout` | 2500 | C# device sync option. No effect in this port. |
| `WifiSyncReceiveTimeout` | 5000 | C# device sync option. No effect in this port. |
| `WifiSyncSendTimeout` | 5000 | C# device sync option. No effect in this port. |

## `[settings]`

The `Settings` object (the old `Config.xml`). Most keys are
Preferences dialog options or reader/browser behavior switches. The
app writes the "app-managed" keys itself.

### Reader behavior

| Key | Default | Meaning |
|---|---|---|
| `AutoNavigateComics` | true | Reading past the start or end opens the next book. |
| `AutoScrolling` | false | Turns autoscrolling on. |
| `AutoShowQuickReview` | false | Shows the Quick Review dialog after finishing a book. |
| `AutoHideMagnifier` | true | The magnifier hides after release. |
| `AutoMagnifier` | true | The magnifier shows automatically. |
| `AutoMinimalGui` | false | Full screen also switches to the minimal UI. |
| `BlendWhilePaging` | false | Blend animation while fast paging. |
| `CurrentPageShowsName` | false | The page overlay shows the page name. |
| `DisplayChangeAnimation` | true | Animates display changes. |
| `FlowingMouseScrolling` | true | Flowing mouse scrolling in the reader. |
| `HardwareAcceleration` | true | Reader hardware acceleration switch (C# parity). |
| `HardwareFiltering` | false | Hardware page filtering (C# parity). |
| `HideCursorFullScreen` | true | Hides the mouse cursor in full screen. |
| `LeftRightMovementReversed` | false | Reverses left and right movement. |
| `MagnifyOpaque` | 1.0 | Magnifier lens opacity. |
| `MagnifySize` | [300, 200] | Magnifier lens size in pixels. |
| `MagnifyStyle` | "Glass" | Magnifier lens style. |
| `MagnifyZoom` | 2.0 | Magnifier zoom factor. |
| `MouseWheelSpeed` | 2.0 | Pages per mouse wheel notch. |
| `NavigationOverlayOnTop` | false | Draws the navigation overlay on top. |
| `OpenLastPage` | true | Opens a book at the page where it was closed. |
| `PageChangeDelay` | true | Mouse wheel and cursor keys delay on page transitions. |
| `PageImageDisplayOptions` | "HighQuality" | Page image render quality. |
| `OverlayScaling` | 100 | Reader overlay scale percent. |
| `ReaderKeyboardMapping` | [] | Custom reader key bindings. |
| `ResetZoomOnPageChange` | false | Zoom resets to 100 percent on page change. |
| `RightToLeftReadingMode` | "FlipPages" | Right to left page flow. |
| `ScrollingDoesBrowse` | true | Scrolling to the page margin turns pages. |
| `ShowCurrentPageOverlay` | true | Shows the current page overlay. |
| `ShowNavigationOverlay` | true | Shows the navigation overlay. |
| `ShowStatusOverlay` | true | Shows the status overlay. |
| `ShowVisiblePagePartOverlay` | true | Shows the visible part overlay. |
| `SmoothScrolling` | true | Smooth scrolling in continuous mode. |
| `SoftwareFiltering` | true | Software page filtering. |
| `TrackCurrentPage` | true | The browser follows the reader page. |
| `TrueRightToLeftReading` | false | True right to left reading. |
| `ZoomInOutOnPageChange` | true | A zoom out plays on page change. |

### Browser

| Key | Default | Meaning |
|---|---|---|
| `AlwaysDisplayBrowserDockingGrip` | false | Always shows the browser docking grip. |
| `CatalogOnlyForFileless` | true | Shows catalog fields only for fileless books. |
| `CommonListStackLayout` | false | All stacks in a list share one layout. |
| `CoverThumbnailsSameSize` | false | All cover thumbnails use the same size. |
| `DisableDragDrop` | false | Disables opening files by drag and drop. |
| `DisplayLibraryGauges` | true | Shows the library gauges. |
| `DogEarThumbnails` | true | Selected thumbnails carry a dog-ear. |
| `FadeInThumbnails` | true | Thumbnails fade in when loaded. |
| `InformationCover3D` | true | 3D cover display in the book info dialog. |
| `LocalQuickSearch` | true | Each list keeps its own quick search settings. |
| `NewBooksChecked` | true | Newly added books are checked. |
| `NumericRatingThumbnails` | true | Thumbnails show numeric ratings. |
| `QuickOpenThumbnailSize` | 128 | Quick Open cover height in pixels. |
| `ShowCustomBookFields` | false | Shows custom book fields. |
| `ShowQuickOpen` | true | Shows Quick Open when no book is open. |
| `ShowSearchLinks` | true | Shows search links. |
| `ShowToolTips` | false | Shows tooltips for books in the browser. |

### Startup and opening

| Key | Default | Meaning |
|---|---|---|
| `AddToLibraryOnOpen` | false | Adds opened files to the library. |
| `CloseBrowserOnOpen` | false | Closes the browser when a book opens. |
| `NewsStartup` | true | Checks for news at boot. |
| `OpenInNewTab` | false | Opens books in a new tab. |
| `OpenLastFile` | true | Reopens the books of the last session. |
| `ScanStartup` | false | Rescans the book folders at boot. |
| `ShowQuickManual` | true | Shows the quick manual at boot. |
| `ShowSplash` | true | Shows the splash screen. |
| `UpdateWebComicsStartup` | false | Updates web comics at boot (web comics are not ported). |

### Caching (Preferences, Advanced page)

| Key | Default | Meaning |
|---|---|---|
| `GenerateThumbnailsOnDemand` | true | Generates cover thumbnails when books display. Port addition. |
| `InternetCacheEnabled` | true | Turns the internet cache on. |
| `InternetCacheSizeMB` | 1000 | Internet cache budget in MB. |
| `MaximumMemoryMB` | 4096 | Overall memory budget in MB. |
| `MemoryPageCacheCount` | 25 | Pages kept in the memory page cache. |
| `MemoryPageCacheOptimized` | true | Optimizes the memory page cache. |
| `MemoryThumbCacheOptimized` | true | Optimizes the memory thumbnail cache. |
| `MemoryThumbCacheSizeMB` | 50 | Memory budget for cached thumbnails in MB. |
| `PageCacheEnabled` | true | Turns the page disk cache on. |
| `PageCacheSizeMB` | 500 | Page disk cache budget in MB. |
| `ThumbCacheEnabled` | true | Turns the thumbnail disk cache on. |
| `ThumbCacheSizeMB` | 500 | Thumbnail disk cache budget in MB. |

### Network, application, files

| Key | Default | Meaning |
|---|---|---|
| `AnimatePanels` | true | Animates panel expand and collapse. |
| `AutoConnectShares` | true | Connects network shares automatically. |
| `AutoUpdateComicsFiles` | false | Writes changes into book files automatically. |
| `CloseMinimizesToTray` | true | Close moves the app to the notification area (C# parity; no tray in this port). |
| `DontAddRemoveFiles` | false | Skips add and remove during scans. |
| `ExportedListsContainFilenames` | false | Exported book lists carry file names. |
| `HelpSystem` | "ComicRack Online Manual" | Which help the Help menu opens. |
| `HiddenMessageBoxes` | "None" | Suppressed message dialogs. |
| `LookForShared` | true | Looks for shared comic libraries on the network. |
| `MinimizeToTray` | false | Minimize moves the app to the notification area (C# parity; no tray in this port). |
| `OverwriteAssociations` | false | Overwrites file associations (Windows). |
| `RemoveMissingFilesOnFullScan` | false | Removes vanished files from the library on a full scan. |
| `Scripting` | true | C# scripting gate. This port has no scripting host. |
| `ScriptingLibraries` | "" | C# scripting libraries. No effect in this port. |
| `HideSampleScripts` | false | C# sample scripts gate. No effect in this port. |
| `ShowMainMenuNoComicOpen` | true | Shows the main menu when no book is open. |
| `UpdateComicBookFiles` | false | Update Book Files writes the extra info (ComicBook.xml). |
| `UpdateComicFiles` | false | Update Book Files writes new info into files. |

### App-managed state

The app writes these keys. Edit them only with care.

| Key | Default | Meaning |
|---|---|---|
| `AlsoRemoveFromLibrary` | false | Remove dialog option memory. |
| `AutoHideMainMenu` | true | C# auto hide menu gate. The port keeps the menubar always visible. |
| `AlsoRemoveFromLibraryFiltered` | false | Remove dialog option memory. |
| `ExplorerIncludeSubFolders` | false | Folders view subfolder toggle. |
| `ExternalServerAddress` | "" | Remote library share address (C# server feature). |
| `ExtraWifiDeviceAddresses` | "" | C# device sync addresses. No effect in this port. |
| `FavoriteFolders` | [] | Folders view favorites. |
| `LastExplorerFolder` | "" | Folders view last folder. |
| `LastExportPageFilterIndex` | 1 | Export page dialog filter index. |
| `LastLibraryItem` | "00000000-0000-0000-0000-000000000000" | Navigator selected list id. |
| `LastOpenFiles` | [] | Open books of the last session. |
| `LastOpenFilterIndex` | 1 | Open dialog filter index. |
| `LastSaveFilterIndex` | 1 | Save dialog filter index. |
| `LibraryGaugesFormat` | "Default" | Library gauges format. |
| `PageFilter` | "All" | Reader page filter. |
| `PasteProperties` | "Series" | Property names the last Copy Properties grabbed. |
| `PrivateListingPassword` | "" | Remote library password (C# server feature). |
| `LibraryQuickSearchList` | [] | Saved library quick searches. |
| `QuickSearchList` | [] | Saved quick searches. |
| `RemoveFilesfromDatabase` | false | Remove dialog option memory. |
| `RunCount` | 0 | Boot counter. |
| `TabLayouts` | "None" | Workspace tab layout memory. |
| `MoveFilesToRecycleBin` | false | Remove dialog trash option memory. |

### Optional `[settings]` members

These members appear only when set.

| Key | Default | Meaning |
|---|---|---|
| `CurrentWorkspace` | (absent) | The saved workspace layout. The app writes it on close. |
| `PluginsStates` | (absent) | Plugin enable states. |
| `SelectedBrowser` | (absent) | Last active browser panel id. |

### `[settings.CurrentWorkspace]`

The saved workspace layout. The app writes it on close. Hand edits
apply at the next start.

```toml
[settings.CurrentWorkspace]        # window bounds, sidebar state
[settings.CurrentWorkspace.View]   # view mode, sort, group, sizes
[[settings.CurrentWorkspace.View.Columns]]  # one table per column (id, visible, width)
[settings.CurrentWorkspace.Reader] # reader fit, layout, rotation, zoom
[settings.CurrentWorkspace.Display]# reader display options
```

## `[plugins.comic-vine-scraper]`

The Comic Vine Scraper settings. `scrape_in_groups` is carried by the
engine but never persisted (C# parity).

| Key | Default | Meaning |
|---|---|---|
| `apiKey` | "" | Comic Vine API key. Required for scraping. |
| `overwriteExisting` | true | Scraped values overwrite existing values. |
| `ignoreBlanks` | false | Blank scrape results do not clear existing values when true. |
| `convertImprints` | true | Converts imprints to the parent publisher. |
| `autochooseSeries` | false | Picks the series automatically when one match scores best. |
| `confirmIssue` | false | Always shows the issue pick dialog. |
| `downloadThumbs` | true | Downloads cover pictures. |
| `preserveThumbs` | true | Keeps existing custom cover pictures. |
| `fastRescrape` | true | Fast rescrape path for already scraped books. |
| `updateNotes` | true | Rescrape writes the scrape info into Notes. |
| `updateTags` | false | Rescrape rewrites the tags. |
| `summaryDialog` | true | Shows the summary dialog after the scrape. |
| `updateSeries` | true | Updates the series. |
| `updateNumber` | true | Updates the issue number. |
| `updatePublished` | true | Updates the published date. |
| `updateReleased` | true | Updates the release date. |
| `updateTitle` | true | Updates the title. |
| `updateCrossovers` | true | Updates the crossovers. |
| `updateWriter` | true | Updates the writer. |
| `updatePenciller` | true | Updates the penciller. |
| `updateInker` | true | Updates the inker. |
| `updateCoverArtist` | true | Updates the cover artist. |
| `updateColorist` | true | Updates the colorist. |
| `updateLetterer` | true | Updates the letterer. |
| `updateEditor` | true | Updates the editor. |
| `updateSummary` | true | Updates the summary. |
| `updateImprint` | true | Updates the imprint. |
| `updatePublisher` | true | Updates the publisher. |
| `updateVolume` | true | Updates the volume. |
| `updateCharacters` | true | Updates the characters. |
| `updateTeams` | true | Updates the teams. |
| `updateLocations` | true | Updates the locations. |
| `updateWebpage` | true | Updates the webpage. |
| `advancedSettings` | "" | The advanced settings lines (next table). |

### `advancedSettings` lines

One `KEY=VALUE` line each. The `-->` form (also `=>`) maps the left
name to the right name.

| Key | Default | Meaning |
|---|---|---|
| `IGNORE_PUBLISHER` | (list) | Publishers the search skips. One per line. |
| `IGNORE_SEARCHTERM` | (list) | Search terms the search skips. One per line. |
| `IGNORE_BEFORE_YEAR` | 0 | Ignores series older than this year. |
| `IGNORE_AFTER_YEAR` | 9999999 | Ignores series newer than this year. |
| `NEVER_IGNORE_THRESHOLD` | 9999999 | A match above this score ignores the year filters. |
| `SCRAPE_RATING` | false | Writes the Comic Vine rating. |
| `SHOW_COVERS` | true | Shows covers in the pick dialogs. |
| `WELCOME_DIALOG` | true | Shows the wizard welcome line. |
| `ALT_SEARCH_REGEX` | "" | Alternate search regex. A bad regex is ignored. |
| `IGNORE_FOLDERS` | false | Ignores folder hints during matching. |
| `FORCE_SERIES_ART` | false | Forces series art over issue art. |
| `NOTE_SCRAPE_DATE` | false | Writes the scrape date into Notes. |
| `PUBLISHER_ALIAS` | (map) | `Publisher --> Alias` lines. The search uses the alias for the publisher. |
| `IMPRINT` | (map) | `Imprint --> Publisher` lines. |
| `SCRAPE_DELAY` | 1 | Delay in seconds between API calls. Clamped to 2-3600. |
| `MAX_SEARCH_RESULTS` | 100 | Maximum search results. Clamped to 10-5000. |
| `CACHE_ENABLED` | true | Uses the Comic Vine disk cache (ADR-037). |
| `CACHE_RATE_LIMIT` | 200 | Requests per API resource per hour. Clamped to 1-100000. The figure comes from a Comic Vine statement, not from the API reference page. |
| `CACHE_CLOSED_HORIZON_DAYS` | 365 | A volume whose last cover date is older than this, and whose issue count matches the cache, is closed. A closed volume makes no request. Clamped to 1-36500. |
| `CACHE_REVALIDATE_HOURS` | 24 | How often an open volume is revalidated. One request. Clamped to 1-8760. |
| `CACHE_WARM_ENABLED` | false | Runs the cache warm task. |
| `CACHE_WARM_MAX_REQUESTS` | 50 | The request cap of one warm run. Clamped to 1-10000. |

The cache file is
`$XDG_DATA_HOME/comicrust/plugins/comic-vine-scraper/cvcache.sqlite`.
It is disposable: delete it and the app rebuilds it, at the cost of API
budget. Import an MCL file to seed it with no request.

## `[data]`

User editable data tables. The app seeds them on first boot.

| Key | Meaning |
|---|---|
| `[data.revision]` | Per table revision markers. Do not edit. |
| `[data.imprints]` | Imprint to publisher mapping. One line per imprint: `"Imprint" = "Publisher"`. |

You can edit or extend `[data.imprints]` by hand. A comicrust update
may add missing entries. Your edits stay.

## Keys not in the file (command line only)

These keys ride the command line only. The file does not list them.

| Switch | Key | Purpose |
|---|---|---|
| `-ac` | `AlternateConfig` | Alternate config directory. |
| `-l` | `Language` | Interface language. |
| `-il` | `ImportList` | Import a reading list file. |
| `-ip` | `InstallPlugin` | Install a plugin. |
| `-ws` | `Workspace` | Select a workspace. |
| `-p` | `Page` | Open a book at a page. |
| (none) | `OwnRemoteConnect` | C# remote feature switch. |
| (none) | `DisableBackupManager` | C# backup manager gate. |
| `-restart` | `Restart` | Internal restart switch. |
| `-waitpid` | `WaitPid` | Internal restart handshake pid. |