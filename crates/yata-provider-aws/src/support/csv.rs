//! A small RFC 4180 CSV parser.
//!
//! The parser is independent of any AWS service. It splits CSV text into a header row and
//! data rows of strings. Values are returned exactly as written (apart from removing the
//! enclosing quotes and un-doubling `""`): surrounding whitespace is kept and no type
//! conversion is done.
//!
//! Rules:
//! - Fields are separated by `,`. Rows end with LF or CRLF.
//! - A field that starts with `"` is quoted. Inside it `,` and line breaks are part of the
//!   value and `""` stands for one `"`. The enclosing quotes are not part of the value.
//! - An unquoted field runs to the next `,` or row end. A `"` inside it is kept as is.
//! - The first row is the header. Zero-length rows are skipped. A leading UTF-8 BOM is removed.
//! - Checks that depend on the meaning of the columns (duplicate names, required columns)
//!   belong to the caller.

// Used by the credential report collector that is added in a later change.
#![allow(dead_code)]

use std::fmt;
use std::iter::Peekable;
use std::str::Chars;

/// Reasons why a CSV text cannot be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CsvError {
    /// A quote is not closed, or the character after a closing quote is not a separator,
    /// a row end or the end of the input.
    InvalidCsv,
    /// There is no header row (the input is empty or has only zero-length rows).
    MissingHeader,
    /// A data row has a different number of fields than the header row.
    FieldCountMismatch,
}

impl CsvError {
    /// Returns the reason code used in failure marks (for example `"InvalidCsv"`).
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::InvalidCsv => "InvalidCsv",
            Self::MissingHeader => "MissingHeader",
            Self::FieldCountMismatch => "FieldCountMismatch",
        }
    }
}

impl fmt::Display for CsvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for CsvError {}

/// A parsed CSV: the header row and the data rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CsvTable {
    /// Column names, in the order of the header row.
    pub(crate) header: Vec<String>,
    /// Data rows. Every row has exactly `header.len()` values.
    pub(crate) rows: Vec<Vec<String>>,
}

/// Parses `text` as CSV into a header row and data rows.
///
/// Returns `MissingHeader` when there is no header row, `InvalidCsv` for broken quoting and
/// `FieldCountMismatch` when a data row does not have as many values as the header row.
/// A header-only input gives a table with no rows.
pub(crate) fn parse_csv(text: &str) -> Result<CsvTable, CsvError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut chars = text.chars().peekable();
    let mut records: Vec<Vec<String>> = Vec::new();

    while chars.peek().is_some() {
        if skip_row_end(&mut chars) {
            continue;
        }
        records.push(parse_record(&mut chars)?);
    }

    let mut records = records.into_iter();
    let header = records.next().ok_or(CsvError::MissingHeader)?;
    let rows: Vec<Vec<String>> = records.collect();
    if rows.iter().any(|row| row.len() != header.len()) {
        return Err(CsvError::FieldCountMismatch);
    }
    Ok(CsvTable { header, rows })
}

/// Consumes a row end (`\n` or `\r\n`) if one is next. Returns whether it did.
fn skip_row_end(chars: &mut Peekable<Chars<'_>>) -> bool {
    match chars.peek() {
        Some('\n') => {
            chars.next();
            true
        }
        Some('\r') => {
            let mut ahead = chars.clone();
            ahead.next();
            if ahead.peek() == Some(&'\n') {
                chars.next();
                chars.next();
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Parses one row, consuming its row end. The input must not be at a row end or the end.
fn parse_record(chars: &mut Peekable<Chars<'_>>) -> Result<Vec<String>, CsvError> {
    let mut fields = Vec::new();
    loop {
        let quoted = chars.peek() == Some(&'"');
        fields.push(if quoted {
            parse_quoted(chars)?
        } else {
            parse_unquoted(chars)
        });
        if chars.peek() == Some(&',') {
            chars.next();
            continue;
        }
        if chars.peek().is_none() || skip_row_end(chars) {
            return Ok(fields);
        }
        // Only a closed quoted field can be followed by something else.
        return Err(CsvError::InvalidCsv);
    }
}

/// Parses a quoted field. The next character must be the opening quote.
fn parse_quoted(chars: &mut Peekable<Chars<'_>>) -> Result<String, CsvError> {
    chars.next();
    let mut value = String::new();
    loop {
        match chars.next() {
            None => return Err(CsvError::InvalidCsv),
            Some('"') => {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    value.push('"');
                } else {
                    return Ok(value);
                }
            }
            Some(c) => value.push(c),
        }
    }
}

/// Parses an unquoted field up to (not including) the next `,`, row end or the end.
fn parse_unquoted(chars: &mut Peekable<Chars<'_>>) -> String {
    let mut value = String::new();
    while let Some(&c) = chars.peek() {
        if c == ',' || c == '\n' {
            break;
        }
        if c == '\r' {
            let mut ahead = chars.clone();
            ahead.next();
            if ahead.peek() == Some(&'\n') {
                break;
            }
        }
        value.push(c);
        chars.next();
    }
    value
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn rows(text: &str) -> Vec<Vec<String>> {
        parse_csv(text).unwrap().rows
    }

    fn strs(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_owned()).collect()
    }

    #[test]
    fn row_separators_and_empty_rows() {
        let expected = vec![strs(&["1", "2"]), strs(&["3", "4"])];
        for text in [
            "a,b\n1,2\n3,4\n",
            "a,b\r\n1,2\r\n3,4\r\n",
            "a,b\n1,2\n3,4",
            "a,b\r\n1,2\r\n3,4",
            "a,b\n\n1,2\n\n\n3,4\n\n",
            "a,b\r\n\r\n1,2\r\n3,4\r\n\r\n",
            "a,b\n1,2\r\n3,4\n",
        ] {
            let table = parse_csv(text).unwrap();
            assert_eq!(table.header, strs(&["a", "b"]), "{text:?}");
            assert_eq!(table.rows, expected, "{text:?}");
        }
    }

    #[test]
    fn leading_bom_is_removed() {
        let table = parse_csv("\u{feff}a,b\n1,2\n").unwrap();
        assert_eq!(table.header, strs(&["a", "b"]));
        assert_eq!(table.rows, vec![strs(&["1", "2"])]);
    }

    #[test]
    fn quoted_values() {
        let cases: &[(&str, &[&str])] = &[
            ("h1,h2\n\"a,b\",c\n", &["a,b", "c"]),
            ("h1,h2\n\"a\nb\",c\n", &["a\nb", "c"]),
            ("h1,h2\n\"a\r\nb\",c\n", &["a\r\nb", "c"]),
            ("h1,h2\n\"say \"\"hi\"\"\",c\n", &["say \"hi\"", "c"]),
            ("h1,h2\n\"\"\"\",c\n", &["\"", "c"]),
            ("h1,h2\n\"\",c\n", &["", "c"]),
            ("h1,h2\n,c\n", &["", "c"]),
            ("h1,h2\na,\"\"\n", &["a", ""]),
            ("h1,h2\na,\n", &["a", ""]),
            ("h1,h2\n\"a\",\"b\"", &["a", "b"]),
        ];
        for (text, expected) in cases {
            assert_eq!(rows(text), vec![strs(expected)], "{text:?}");
        }
    }

    #[test]
    fn quoted_header_names() {
        let table = parse_csv("\"a,1\",\"b\"\"2\"\nx,y\n").unwrap();
        assert_eq!(table.header, strs(&["a,1", "b\"2"]));
    }

    #[test]
    fn quoted_empty_row_is_not_skipped() {
        let table = parse_csv("a\n\"\"\n").unwrap();
        assert_eq!(table.rows, vec![strs(&[""])]);
    }

    #[test]
    fn whitespace_is_kept() {
        assert_eq!(rows("a,b\n  x ,\ty\t\n"), vec![strs(&["  x ", "\ty\t"])]);
        assert_eq!(rows("a,b\n\" x \", \n"), vec![strs(&[" x ", " "])]);
    }

    #[test]
    fn quote_inside_unquoted_value_is_kept() {
        assert_eq!(rows("a,b\na\"b,c\"\n"), vec![strs(&["a\"b", "c\""])]);
    }

    #[test]
    fn bare_carriage_return_is_part_of_value() {
        assert_eq!(rows("a,b\nx\ry,z\n"), vec![strs(&["x\ry", "z"])]);
    }

    #[test]
    fn carriage_return_at_the_end_of_input_is_part_of_value() {
        assert_eq!(rows("a\n1\r"), vec![strs(&["1\r"])]);
    }

    #[test]
    fn carriage_return_after_a_closing_quote_is_invalid() {
        assert_eq!(parse_csv("a,b\n\"abc\"\r,d\n"), Err(CsvError::InvalidCsv));
    }

    #[test]
    fn invalid_quoting_takes_precedence_over_field_count_mismatch() {
        assert_eq!(parse_csv("a,b\n1\n\"x"), Err(CsvError::InvalidCsv));
    }

    #[test]
    fn invalid_quoting() {
        for text in [
            "a,b\n\"abc,d\n",
            "a,b\n\"abc",
            "a,b\n\"abc\"x,d\n",
            "a,b\n\"abc\" ,d\n",
            "a,b\nx,\"abc\"\"\n",
            "\"a",
        ] {
            assert_eq!(parse_csv(text), Err(CsvError::InvalidCsv), "{text:?}");
        }
    }

    #[test]
    fn missing_header() {
        for text in ["", "\n", "\r\n", "\n\n\r\n", "\u{feff}", "\u{feff}\n"] {
            assert_eq!(parse_csv(text), Err(CsvError::MissingHeader), "{text:?}");
        }
    }

    #[test]
    fn field_count_mismatch() {
        for text in [
            "a,b\n1\n",
            "a,b\n1,2,3\n",
            "a,b\n1,2\n3\n",
            "a,b\n\"1\",2,\n",
        ] {
            assert_eq!(
                parse_csv(text),
                Err(CsvError::FieldCountMismatch),
                "{text:?}"
            );
        }
    }

    #[test]
    fn header_only_gives_no_rows() {
        for text in ["a,b", "a,b\n", "a,b\r\n\r\n"] {
            let table = parse_csv(text).unwrap();
            assert_eq!(table.header, strs(&["a", "b"]), "{text:?}");
            assert!(table.rows.is_empty(), "{text:?}");
        }
    }

    #[test]
    fn duplicate_header_names_are_not_checked() {
        let table = parse_csv("a,a\n1,2\n").unwrap();
        assert_eq!(table.header, strs(&["a", "a"]));
    }

    #[test]
    fn error_codes() {
        assert_eq!(CsvError::InvalidCsv.code(), "InvalidCsv");
        assert_eq!(CsvError::MissingHeader.to_string(), "MissingHeader");
        assert_eq!(CsvError::FieldCountMismatch.code(), "FieldCountMismatch");
    }
}
