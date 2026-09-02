//! Hand-rolled XML emission and event-based reading.
//!
//! The writer reproduces `XmlSerializer.Serialize(Stream, object)` on
//! .NET Framework 4.8 byte for byte:
//!
//! - declaration: `<?xml version="1.0" encoding="utf-8"?>`
//! - root carries `xmlns:xsi` then `xmlns:xsd`
//! - indentation: two spaces per level, line separator `\r\n`
//! - no trailing newline, no BOM
//! - empty element form: `<Name />`
//! - members equal to their `[DefaultValue]` are omitted (caller decides)
//! - attributes before elements, base-class members first, declaration
//!   order otherwise

pub mod reader;
pub mod scalar;

use std::io::{self, Write};

pub use reader::{Start, Tok, XmlError, XmlReader, XmlResult};

/// Byte-exact XML writer mirroring the net48 `XmlTextWriter` defaults
/// used by `XmlSerializer`.
pub struct Emitter<W: Write> {
    out: W,
    stack: Vec<Elem>,
}

struct Elem {
    name: String,
    /// A child element or raw fragment was written.
    has_children: bool,
    /// Text content was written (closes inline, no newline).
    has_text: bool,
}

impl<W: Write> Emitter<W> {
    /// Creates an emitter and writes the XML declaration (no newline;
    /// the first element start supplies the line break).
    pub fn new(mut out: W) -> io::Result<Self> {
        out.write_all(b"<?xml version=\"1.0\" encoding=\"utf-8\"?>")?;
        Ok(Emitter {
            out,
            stack: Vec::new(),
        })
    }

    /// Starts the root element with the default `xsi`/`xsd` namespaces.
    pub fn root(&mut self, name: &str) -> io::Result<()> {
        self.start(name)?;
        self.attr("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance")?;
        self.attr("xmlns:xsd", "http://www.w3.org/2001/XMLSchema")
    }

    fn indent(&mut self) -> io::Result<()> {
        self.out.write_all(b"\r\n")?;
        for _ in 0..self.stack.len() {
            self.out.write_all(b"  ")?;
        }
        Ok(())
    }

    /// Starts an element; writes the pending `>` of the parent first.
    pub fn start(&mut self, name: &str) -> io::Result<()> {
        if let Some(parent) = self.stack.last_mut() {
            if !parent.has_children && !parent.has_text {
                self.out.write_all(b">")?;
            }
            parent.has_children = true;
        }
        self.indent()?;
        write!(self.out, "<{name}")?;
        self.stack.push(Elem {
            name: name.to_string(),
            has_children: false,
            has_text: false,
        });
        Ok(())
    }

    /// Writes an attribute on the currently open element.
    pub fn attr(&mut self, name: &str, value: &str) -> io::Result<()> {
        write!(self.out, " {}=\"{}\"", name, escape_attr(value))
    }

    /// Writes text content of the currently open element.
    pub fn text(&mut self, text: &str) -> io::Result<()> {
        let elem = self.stack.last_mut().expect("text outside element");
        if !elem.has_children && !elem.has_text {
            self.out.write_all(b">")?;
        }
        elem.has_text = true;
        self.out.write_all(escape_text(text).as_bytes())
    }

    /// Writes a pre-formed XML fragment (for `[XmlAnyElement]` capture),
    /// indented like a child element.
    pub fn raw(&mut self, xml: &str) -> io::Result<()> {
        let elem = self.stack.last_mut().expect("raw outside element");
        if !elem.has_children && !elem.has_text {
            self.out.write_all(b">")?;
        }
        elem.has_children = true;
        self.indent()?;
        self.out.write_all(xml.as_bytes())
    }

    /// Closes the innermost element.
    pub fn end(&mut self) -> io::Result<()> {
        let elem = self.stack.pop().expect("end without start");
        if elem.has_children {
            self.out.write_all(b"\r\n")?;
            for _ in 0..self.stack.len() {
                self.out.write_all(b"  ")?;
            }
            write!(self.out, "</{}>", elem.name)
        } else if elem.has_text {
            write!(self.out, "</{}>", elem.name)
        } else {
            self.out.write_all(b" />")
        }
    }

    /// Convenience: element with text content, `<Name>text</Name>`.
    pub fn text_elem(&mut self, name: &str, text: &str) -> io::Result<()> {
        self.start(name)?;
        self.text(text)?;
        self.end()
    }

    /// Flushes and returns the inner writer. All elements must be closed.
    pub fn finish(mut self) -> io::Result<W> {
        while !self.stack.is_empty() {
            self.end()?;
        }
        self.out.flush()?;
        Ok(self.out)
    }
}

/// Escapes a text node like `XmlTextWriter` (`&`, `<`, `>`).
pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// Escapes an attribute value like `XmlTextWriter`
/// (`&`, `<`, `>`, `"`, control whitespace).
pub fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            '\t' => out.push_str("&#x9;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emit(f: impl FnOnce(&mut Emitter<Vec<u8>>) -> io::Result<()>) -> String {
        let mut e = Emitter::new(Vec::new()).unwrap();
        f(&mut e).unwrap();
        String::from_utf8(e.finish().unwrap()).unwrap()
    }

    #[test]
    fn empty_document_form() {
        let out = emit(|e| {
            e.root("ComicDatabase")?;
            e.end()
        });
        assert_eq!(
            out,
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n\
             <ComicDatabase xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" \
             xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" />"
        );
    }

    #[test]
    fn nesting_and_indent() {
        let out = emit(|e| {
            e.root("R")?;
            e.attr("Id", "x")?;
            e.start("A")?;
            e.text_elem("B", "hi")?;
            e.start("C")?;
            e.end()?;
            e.end()?;
            e.end()
        });
        assert_eq!(
            out,
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n\
             <R xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" \
             xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" Id=\"x\">\r\n  \
             <A>\r\n    <B>hi</B>\r\n    <C />\r\n  </A>\r\n</R>"
        );
    }

    #[test]
    fn text_escaping() {
        let out = emit(|e| {
            e.start("T")?;
            e.text("a<b>&c\"d")?;
            e.end()
        });
        assert_eq!(
            out,
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<T>a&lt;b&gt;&amp;c\"d</T>"
        );
        let out = emit(|e| {
            e.start("T")?;
            e.attr("a", "x\"y&z")?;
            e.end()
        });
        assert_eq!(
            out,
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<T a=\"x&quot;y&amp;z\" />"
        );
    }
}
