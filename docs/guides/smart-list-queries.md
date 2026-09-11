# Guide: smart-list queries

A smart list is a saved query. This guide gives the query language, the
full list of fields you can match on, the operators each field accepts,
and worked examples.

Everything here is taken from the matcher registry in
`crates/cr-engine/src/matcher/spec.rs` and the evaluator in
`eval.rs`. `crates/cr-engine/tests/query_doc.rs` reads this file, parses
every example, and fails when a field name in the tables below is not in
the registry, or when a registry field is missing from the tables. The
document cannot drift from the code without breaking the build.

## The query language

```text
[Name "<list name>"]
[[Not] In [<base list>]]
Match [All|Any] [{] <rule> [, <rule> ...] [}]
```

A rule is:

```text
[Not] [<Field>] <operator> ["<value>" ["<value 2>"]]
```

Rules:

- The field name goes in square brackets. It is case-insensitive.
- Values go in double quotes. Escape a quote with `\"`.
- Escape a `[` or `]` inside a field name with `\[` and `\]`.
- `All` means every rule must match. `Any` means one is enough.
- `All` is the default. With one rule you can leave the word out.
- Braces are needed when there is more than one rule.
- `Not` before a rule inverts that rule.
- `Not` before a nested `Match` inverts the whole group.
- A rule can be a nested `Match` group, to any depth.

### The `Name` and `In` prelude

The parser accepts a `Name "..."` and an `In [...]` prelude, because
ComicRack query text carries them. **The editor throws both away when
you press OK.** It keeps only the `Match` part.

So do not set these by typing them:

- The list name comes from the name box in the editor.
- The base list comes from the base-list control in the editor, with
  its own "not in base list" tick box.

A base list narrows the query to the books of another list. "Not in
base list" does the opposite and excludes them. Both work, but you set
them with the controls, not in the query text.

## Operators

The operators a field accepts depend on its type. The type of every
field is in the reference tables further down.

### Text fields

| Operator | Values | Meaning |
|---|---|---|
| `equals` | 1 | The whole field is exactly this text. |
| `contains` | 1 | The text appears anywhere in the field. |
| `contains any of` | 1 | The field contains at least one of the listed words. |
| `contains all of` | 1 | The field contains every listed word. |
| `starts with` | 1 | The field begins with this text. |
| `ends with` | 1 | The field ends with this text. |
| `list contains` | 1 | The field is a comma or semicolon list, and one member equals this text. |
| `regex` | 1 | A .NET-style regular expression matches somewhere in the field. |

Text rules ignore case.

For `contains any of` and `contains all of`, separate the words with
commas or semicolons. When the value has no comma and no semicolon, it
is split on spaces instead. So a value with spaces in it needs a comma
somewhere, or it becomes several words.

Two traps:

- `contains ""` matches EVERY book. An empty search value is treated as
  "no filter", not as "is empty". The same is true for `starts with ""`
  and `ends with ""`.
- There is no "is empty" or "is not empty" operator. To find books where
  a field has any content, use `regex` with the pattern `.`, which needs
  at least one character. Read the next section first: for some fields
  that pattern matches almost everything.

### Fields that fall back to the file name

Seven fields do not read the stored metadata alone. When the stored
value is empty, and the book has "use proposed values" on, which is the
default, the value comes from parsing the FILE NAME instead:

| Field | Falls back to |
|---|---|
| `Series` | The series read from the file name. |
| `Title` | The title read from the file name. |
| `Format` | The format word read from the file name. |
| `Number` | The issue number read from the file name. |
| `Volume` | The volume read from the file name. |
| `Count` | The issue count read from the file name. |
| `Year` | The year read from the file name. |

This matters most when you search for MISSING metadata. A book with an
empty `Series` field still matches `[Series] regex "."` when its file
name gives a series, which is nearly always. To find books whose stored
metadata is genuinely blank, use a field that has no fallback, such as
`Summary`, `Writer`, `Publisher` or `Notes`.

### Number fields

| Operator | Values | Meaning |
|---|---|---|
| `equals` | 1 | Exactly this number. |
| `is greater` | 1 | Larger than this number. |
| `is smaller` | 1 | Smaller than this number. |
| `in range` | 2 | Between the two numbers, and both ends count. |

A value that is not a number counts as `-1`.

### Date fields

| Operator | Values | Meaning |
|---|---|---|
| `equals` | 1 | The same day. |
| `is after` | 1 | Later than this day. |
| `is before` | 1 | Earlier than this day. |
| `is in last days` | 1 | Within this many days before now. |
| `is in range` | 2 | Between the two days, and both ends count. |

The time of day is always ignored. A date value can be written as:

- `2024-05-17` (year, month, day)
- `05/17/2024` (month, day, year)
- a plain number, which means "that many days ago"

### Yes/no fields

| Operator | Values |
|---|---|
| `equals yes` | 0 |
| `equals no` | 0 |
| `equals unknown` | 0 |

The `Manga` field adds `equals ltr` for left-to-right manga.

The `Only Duplicates` field uses `on` and `off` instead.

## Field reference

### Text fields

| Field | What it matches |
|---|---|
| `Age Rating` | The age rating. |
| `All` | Every text field at once, plus the custom values. |
| `Alternate Series` | The alternate series name. |
| `Book Age` | The `Book Age` property. |
| `Book Collection Status` | The collection status. |
| `Book Condition` | The condition. |
| `Book Location` | Where the physical book is. |
| `Book Notes` | Your notes on the book. |
| `Book Owner` | The owner. |
| `Book Store` | Where it was bought. |
| `Characters` | The character list. |
| `Colorist` | The colorist credit. |
| `Custom Value` | A named custom value. Takes TWO values: the key, then the text to compare. |
| `Editor` | The editor credit. |
| `File Format` | The reader format name, for example `eComic (ZIP)` or `eComic (RAR)`. |
| `File` | The file name. |
| `File Directory` | The folder holding the file. |
| `File Path` | The whole path. |
| `Format` | The `Format` metadata field, for example `Annual` or `TPB`. |
| `Genre` | The genre list. |
| `Imprint` | The imprint. |
| `Inker` | The inker credit. |
| `ISBN` | The ISBN. |
| `Language` | The language code. |
| `Letterer` | The letterer credit. |
| `Locations` | The location list. |
| `Main Character/Team` | The main character or team. |
| `Notes` | The metadata notes field. |
| `Penciller` | The penciller credit. |
| `Publisher` | The publisher. |
| `Review` | Your review text. |
| `Scanning Information` | The scan credit text in the metadata. |
| `Series` | The series name. |
| `Series Group` | The series group. |
| `Story Arc` | The story arc. |
| `Summary` | The summary text. |
| `Tags` | The tag list. |
| `Teams` | The team list. |
| `Title` | The issue title. |
| `Translator` | The translator credit. |
| `Web` | The web link. |
| `Writer` | The writer credit. |

### Number fields

| Field | What it matches |
|---|---|
| `Alternate Count` | The alternate issue count. |
| `Alternate Number` | The alternate issue number. |
| `Bookmark Count` | Always 0. The bookmark count is not ported yet, so any rule on this field compares against 0. |
| `Book Price` | The price you recorded. |
| `Community Rating` | The community rating. |
| `Count` | The issue count of the series. |
| `Day` | The cover day. |
| `File Size` | The file size in MEGABYTES, not bytes. |
| `Month` | The cover month, 1 to 12. |
| `New Pages` | The count of pages marked new. |
| `Number` | The issue number. |
| `Page Count` | The page count. |
| `My Rating` | Your rating, 0 to 5. |
| `Read Percentage` | How much you read, 0 to 100. |
| `Volume` | The volume. |
| `Week` | The cover week. |
| `Year` | The cover year. |

### Date fields

| Field | What it matches |
|---|---|
| `Added` | When the book entered the library. |
| `File Created` | The file creation time. |
| `File Modified` | The file modification time. |
| `Opened` | When you last opened it. |
| `Published` | The cover date. |
| `Released` | The release date. |

### Yes/no fields

| Field | What it matches |
|---|---|
| `Black and White` | The book is black and white. |
| `Is Checked` | The book is checked. |
| `Has Custom Values` | The book has at least one custom value. |
| `Is Linked` | The book points at a file. |
| `Is Missing` | The file is gone from disk. |
| `Modified Info` | The metadata changed and is not written back yet. |
| `Modified Library Info` | The library metadata changed. |
| `Series complete` | The series is marked complete. |
| `Manga` | Manga reading direction. Adds `equals ltr`. |
| `Only Duplicates` | Keeps only duplicates. Uses `on` and `off`. |

### Series fields

These ask about the whole series a book belongs to, not the single book.

| Field | Type |
|---|---|
| `Series: All complete` | Yes/no |
| `Series: Average Community Rating` | Number |
| `Series: Average Rating` | Number |
| `Series: Book Count` | Number |
| `Series: Biggest Gap` | Number |
| `Series: End of Gap` | Yes/no |
| `Series: First Number` | Number |
| `Series: First Year` | Number |
| `Series: Gaps` | Number |
| `Series: Highest Count` | Number |
| `Series: Last Number` | Number |
| `Series: Last Year` | Number |
| `Series: Lowest Count` | Number |
| `Series: Opened` | Date |
| `Series: Pages` | Number |
| `Series: Pages Read` | Number |
| `Series: Percent Read` | Number |
| `Series: Published` | Date |
| `Series: Book added` | Date |
| `Series: Book released` | Date |
| `Series: Running Time Years` | Number |
| `Series: Start of Gap` | Yes/no |

### Script fields

| Field | Type | State |
|---|---|---|
| `Expression` | `is true` / `is false`, 1 value | Parses and saves, but never matches. |
| `User Scripts` | `None`, 0 values | Parses and saves, but never matches. |

comicrust has no scripting host (ADR-027). These two fields exist so an
imported ComicRack query keeps its shape and saves back unchanged. A
rule that uses them matches no books.

## Examples

### Simple

Every Batman book:

```text
Match [Series] equals "Batman"
```

Anything published by Marvel:

```text
Match [Publisher] contains "Marvel"
```

Books whose file is gone:

```text
Match [Is Missing] equals yes
```

Books you rated above 3:

```text
Match [My Rating] is greater "3"
```

Books added in the last month:

```text
Match [Added] is in last days "30"
```

Books from the 2000s:

```text
Match [Year] in range "2000" "2009"
```

Books tagged `favorite`, where `Tags` is a comma list:

```text
Match [Tags] list contains "favorite"
```

Everything except Batman:

```text
Match Not [Series] equals "Batman"
```

### Medium

Unread Batman books from 2011 onward:

```text
Match All { [Series] equals "Batman", [Year] is greater "2010", [Read Percentage] equals "0" }
```

Marvel or DC, nothing else:

```text
Match Any { [Publisher] equals "Marvel", [Publisher] equals "DC Comics" }
```

Books written by Morrison or Moore:

```text
Match [Writer] contains any of "Morrison,Moore"
```

Started but not finished:

```text
Match [Read Percentage] in range "1" "99"
```

Big files that may be worth re-compressing. `File Size` counts in
megabytes, so this finds books over 100 MB:

```text
Match All { [File Size] is greater "100", [File Format] equals "eComic (ZIP)" }
```

Books with no summary text, using the regex trick for "is not empty".
`Summary` has no file-name fallback, so a blank result is really blank:

```text
Match Not [Summary] regex "."
```

Books the scan could not read:

```text
Match [Custom Value] equals "comicrust.scan.status" "Unreadable"
```

Any book the scan marked, of any kind:

```text
Match [Custom Value] regex "comicrust.scan.status" "."
```

Series with holes in the numbering:

```text
Match [Series: Gaps] is greater "0"
```

### Complicated

Marvel or DC, present on disk, added this year, excluding annuals and
collections:

```text
Match All { [Is Missing] equals no, [Added] is in last days "365", Match Any { [Publisher] equals "Marvel", [Publisher] equals "DC Comics" }, Not Match Any { [Format] equals "Annual", [Format] equals "TPB" } }
```

Reading backlog: owned, readable, not started, from a series you already
began, and not something you marked as skipped:

```text
Match All { [Is Linked] equals yes, [Is Missing] equals no, [Read Percentage] equals "0", [Series: Percent Read] is greater "0", Not [Tags] list contains "skip" }
```

Series worth completing: incomplete, you liked what you read, and there
is a real gap rather than one missing issue:

```text
Match All { [Series: All complete] equals no, [Series: Biggest Gap] is greater "1", [Series: Average Rating] is greater "3", [Series: Book Count] is greater "2" }
```

Files that need work after a scan: unreadable, timed out, or skipped,
but not the mislabeled ones, which read fine:

```text
Match All { [Custom Value] contains any of "comicrust.scan.status" "Unreadable,Timed out,Skipped", Not [Custom Value] equals "comicrust.scan.status" "Format mismatch" }
```

Metadata cleanup: a linked book missing the fields a scraper fills.
Every field here is one WITHOUT a file-name fallback, so a blank result
really is blank:

```text
Match All { [Is Linked] equals yes, [Is Missing] equals no, Match Any { Not [Writer] regex ".", Not [Publisher] regex ".", Not [Summary] regex "." } }
```

A book with no issue number. `Number` is a number field, and an empty
or unparsable number reads as `-1`. This also catches a file name that
gave no number:

```text
Match [Number] is smaller "0"
```

Find the CBR files that carry a `.cbz` name and are therefore
mislabeled:

```text
Match All { [File Path] regex "(?i)\\.cbz$", [File Format] equals "eComic (RAR)" }
```

Reading state, dates and series statistics together. To limit this to
an existing list, pick that list in the base-list control rather than
typing `In [...]`:

```text
Match All { [Added] is in last days "365", [Read Percentage] is smaller "100", Match Any { [Series: Percent Read] is greater "50", [My Rating] is greater "3" }, Not [Is Missing] equals yes }
```

## Notes

- The editor writes queries in a formatted, multi-line shape. The
  single-line form here is the same language and parses identically.
- Saving a list rewrites the query text from the parsed rules. Comments
  and spacing you type by hand do not survive a save.
- Field names, operator words, and the scan status texts are part of
  saved queries. They are kept stable on purpose.
