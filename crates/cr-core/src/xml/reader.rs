//! Event-based XML reading on top of `quick-xml`.
//!
//! Tolerant like the .NET `XmlSerializer` reader: element order does not
//! matter on load, unknown elements are captured raw (for
//! `[XmlAnyElement]` round-trip), unknown attributes are dropped.

use std::io::BufRead;

use quick_xml::events::Event;

use crate::xml::scalar::ScalarError;

/// A parse error carrying the XML path context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlError(pub String);

impl std::fmt::Display for XmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for XmlError {}

pub type XmlResult<T> = Result<T, XmlError>;

impl From<ScalarError> for XmlError {
    fn from(e: ScalarError) -> Self {
        XmlError(e.0)
    }
}

/// Element start token: name plus decoded attributes in document order.
#[derive(Debug, Clone)]
pub struct Start {
    pub name: String,
    pub attrs: Vec<(String, String)>,
}

impl Start {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// One content token. Text tokens that are pure indentation between
/// elements are never produced.
#[derive(Debug, Clone)]
pub enum Tok {
    Start(Start),
    End(String),
    Text(String),
    Eof,
}

/// Streaming token reader. `expand_empty_elements` is on, so
/// `<Pages />` arrives as Start+End.
pub struct XmlReader<'a> {
    reader: quick_xml::Reader<&'a mut dyn BufRead>,
    buf: Vec<u8>,
    after_start: bool,
    /// Synthetic end tag for self-closing `<X />` (safety net; the
    /// configured `expand_empty_elements` makes this rare).
    pending_end: Option<String>,
    done: bool,
}

impl<'a> XmlReader<'a> {
    pub fn new(reader: &'a mut dyn BufRead) -> Self {
        let mut reader = quick_xml::Reader::from_reader(reader);
        reader.config_mut().expand_empty_elements = true;
        XmlReader {
            reader,
            buf: Vec::new(),
            after_start: false,
            pending_end: None,
            done: false,
        }
    }

    /// Returns the next content token, skipping inter-element
    /// whitespace, comments, processing instructions and declarations.
    pub fn next_tok(&mut self) -> XmlResult<Tok> {
        if self.done {
            return Ok(Tok::Eof);
        }
        if let Some(name) = self.pending_end.take() {
            self.after_start = false;
            return Ok(Tok::End(name));
        }
        loop {
            let decoder = self.reader.decoder();
            let ev = self
                .reader
                .read_event_into(&mut self.buf)
                .map_err(|e| XmlError(format!("xml read error: {e}")))?;
            match ev {
                Event::Eof => {
                    self.done = true;
                    return Ok(Tok::Eof);
                }
                Event::Start(bs) => {
                    self.after_start = true;
                    return Ok(Tok::Start(parse_start(&bs, decoder)?));
                }
                Event::End(be) => {
                    self.after_start = false;
                    return Ok(Tok::End(
                        String::from_utf8_lossy(be.name().as_ref()).into_owned(),
                    ));
                }
                Event::Text(t) => {
                    let text = t
                        .unescape()
                        .map_err(|e| XmlError(format!("text decode: {e}")))?
                        .into_owned();
                    // Whitespace-only text is indentation between
                    // elements (ComicRack output never has
                    // whitespace-only element values inline).
                    if text.trim().is_empty() {
                        continue;
                    }
                    self.after_start = false;
                    return Ok(Tok::Text(text));
                }
                Event::CData(t) => {
                    self.after_start = false;
                    return Ok(Tok::Text(String::from_utf8_lossy(t.as_ref()).into_owned()));
                }
                Event::Empty(bs) => {
                    let start = parse_start(&bs, decoder)?;
                    self.pending_end = Some(start.name.clone());
                    self.after_start = true;
                    return Ok(Tok::Start(start));
                }
                Event::Comment(_) | Event::PI(_) | Event::Decl(_) | Event::DocType(_) => {}
            }
        }
    }
}

fn parse_start(
    bs: &quick_xml::events::BytesStart<'_>,
    decoder: quick_xml::encoding::Decoder,
) -> XmlResult<Start> {
    let name = String::from_utf8_lossy(bs.name().as_ref()).into_owned();
    let mut attrs = Vec::new();
    for a in bs.attributes().with_checks(false) {
        let a = a.map_err(|e| XmlError(format!("attr parse: {e}")))?;
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let value = a
            .decode_and_unescape_value(decoder)
            .map_err(|e| XmlError(format!("attr decode: {e}")))?
            .into_owned();
        attrs.push((key, value));
    }
    Ok(Start { name, attrs })
}

impl<'a> XmlReader<'a> {
    /// Reads the text content of the current element and consumes its
    /// end tag.
    pub fn text_content(&mut self, elem: &str) -> XmlResult<String> {
        match self.next_tok()? {
            Tok::Text(t) => match self.next_tok()? {
                Tok::End(name) if name == elem => Ok(t),
                other => Err(XmlError(format!("expected </{elem}>, got {other:?}"))),
            },
            Tok::End(name) if name == elem => Ok(String::new()),
            other => Err(XmlError(format!(
                "expected text in <{elem}>, got {other:?}"
            ))),
        }
    }

    /// Captures the raw source of the element just started (including
    /// the start tag) as an XML string, consuming through its end tag.
    pub fn capture_raw(&mut self, start: &Start) -> XmlResult<String> {
        let mut out = String::from("<");
        out.push_str(&start.name);
        for (k, v) in &start.attrs {
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            out.push_str(&quick_xml::escape::escape(v));
            out.push('"');
        }
        out.push('>');
        let mut depth = 1usize;
        loop {
            match self.next_tok()? {
                Tok::Eof => return Err(XmlError("unexpected eof in capture".into())),
                Tok::Start(s) => {
                    depth += 1;
                    out.push_str(&format_start_raw(&s));
                }
                Tok::End(name) => {
                    depth -= 1;
                    out.push_str(&format!("</{name}>"));
                    if depth == 0 {
                        return Ok(out);
                    }
                }
                Tok::Text(t) => {
                    out.push_str(&quick_xml::escape::escape(&t));
                }
            }
        }
    }

    /// Consumes tokens until the end tag of `name` at the current depth.
    pub fn skip_element(&mut self, name: &str) -> XmlResult<()> {
        let mut depth = 1usize;
        loop {
            match self.next_tok()? {
                Tok::Eof => return Err(XmlError(format!("unexpected eof in <{name}>"))),
                Tok::Start(_) => depth += 1,
                Tok::End(_) => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Tok::Text(_) => {}
            }
        }
    }
}

fn format_start_raw(s: &Start) -> String {
    let mut out = String::from("<");
    out.push_str(&s.name);
    for (k, v) in &s.attrs {
        out.push_str(&format!(" {}=\"{}\"", k, quick_xml::escape::escape(v)));
    }
    out.push('>');
    out
}
