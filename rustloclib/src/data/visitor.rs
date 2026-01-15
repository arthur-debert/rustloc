//! Rust source file visitor for LOC counting.
//!
//! This module provides the core parsing logic for analyzing Rust source files
//! and categorizing lines into one of 6 types:
//!
//! - **code**: Logic lines in production code
//! - **tests**: Logic lines in test code (#[test], #[cfg(test)], tests/)
//! - **examples**: Logic lines in example code (examples/)
//! - **docs**: Documentation comments (///, //!, /** */, /*! */)
//! - **comments**: Regular comments (//, /* */)
//! - **blanks**: Blank/whitespace-only lines
//!
//! ## Acknowledgment
//!
//! The parsing logic in this module is adapted from
//! [cargo-warloc](https://github.com/Maximkaaa/cargo-warloc) by Maxim Gritsenko.
//! Many thanks to the original author for the excellent implementation.
//! cargo-warloc is licensed under MIT.

use std::fs::File;
use std::io::{BufReader, Read};
use std::mem;
use std::path::Path;

use utf8_chars::BufReadCharsExt;

use crate::error::RustlocError;
use crate::Result;

use super::stats::Locs;

/// A visitor that parses Rust source files and counts lines of code.
///
/// The visitor uses a token-based parser with single-character lookahead to
/// distinguish between code, comments, documentation, and blank lines.
/// It also recognizes test blocks (`#[test]`, `#[cfg(test)]`) and categorizes
/// logic lines appropriately as code, tests, or examples.
pub struct Visitor<T: Read> {
    reader: BufReader<T>,
    context: VisitorContext,
    stats: Locs,
    lookahead: Option<char>,
    curr_string: String,
    curr_line_no: usize,
    debug: bool,
}

/// The context in which code is being analyzed.
///
/// This determines how logic lines are categorized (code/tests/examples).
/// Comments, docs, and blanks are context-independent.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum VisitorContext {
    /// Production code
    Code,
    /// Test code (in `#[test]` or `#[cfg(test)]` blocks, or `tests/` directory)
    Tests,
    /// Example code (in `examples/` directory)
    Example,
}

/// Tracks what kind of content has been seen on the current line.
#[derive(Default, Debug, Copy, Clone)]
struct LineContext {
    has_code: bool,
    has_comment_start: bool,
    has_doc_comment_start: bool,
}

impl LineContext {
    fn is_inside_comment(&self) -> bool {
        self.has_comment_start || self.has_doc_comment_start
    }
}

/// Tokens recognized by the parser.
#[derive(Debug, Eq, PartialEq)]
enum Token {
    LineBreak,
    WhiteSpace,
    TestBlockStart,
    CodeBlockOpen,
    CodeBlockClose,
    CommentStart,
    DocCommentStart,
    CommentBlockOpen,
    CommentBlockClose,
    DocCommentBlockOpen,
    EndOfStatement,
    DoubleBackSlash,
    DoubleQuote,
    EscapedDoubleQuote,
    StringBlockOpen,
    StringBlockClose,
    DoubleStringBlockOpen,
    DoubleStringBlockClose,
    Other,
}

impl VisitorContext {
    /// Determine the context from a file path.
    ///
    /// - Files under `tests/` or named `tests.rs` → Tests
    /// - Files under `examples/` → Example
    /// - Everything else → Code
    pub fn from_file_path(path: impl AsRef<Path>) -> Self {
        for component in path.as_ref().components() {
            match component {
                std::path::Component::Normal(os_str)
                    if os_str == "tests" || os_str == "tests.rs" =>
                {
                    return Self::Tests;
                }
                std::path::Component::Normal(os_str) if os_str == "examples" => {
                    return Self::Example;
                }
                _ => {}
            }
        }

        Self::Code
    }
}

impl Visitor<File> {
    /// Create a new visitor for a file at the given path.
    ///
    /// The context (code/tests/examples) is automatically determined from the path.
    pub fn new(file_path: impl AsRef<Path>, debug: bool) -> Result<Self> {
        let path = file_path.as_ref();
        let file = File::open(path).map_err(|e| RustlocError::FileRead {
            path: path.to_path_buf(),
            source: e,
        })?;
        let mut reader = BufReader::new(file);
        let context = VisitorContext::from_file_path(path);

        let lookahead = reader.chars().next().and_then(|c| c.ok());

        Ok(Self {
            reader,
            context,
            stats: Locs::default(),
            lookahead,
            curr_string: String::new(),
            curr_line_no: 1,
            debug,
        })
    }
}

impl<T: Read> Visitor<T> {
    /// Create a visitor from any reader with a specified context.
    ///
    /// This is useful for testing without actual files.
    pub fn from_reader(reader: T, context: VisitorContext, debug: bool) -> Self {
        let mut reader = BufReader::new(reader);
        let lookahead = reader.chars().next().and_then(|c| c.ok());

        Self {
            reader,
            context,
            stats: Locs::default(),
            lookahead,
            curr_string: String::new(),
            curr_line_no: 1,
            debug,
        }
    }

    /// Visit the file and return LOC statistics.
    pub fn visit_file(mut self) -> Locs {
        self.visit_code(self.context);
        self.stats
    }

    fn visit_code(&mut self, context: VisitorContext) {
        let line_context = LineContext::default();
        self.visit_code_block(context, line_context, true);
    }

    fn visit_test_block(&mut self) {
        self.skip_line(
            VisitorContext::Tests,
            LineContext {
                has_code: true,
                ..Default::default()
            },
        );

        let mut line_context = LineContext::default();

        while let Some(token) = self.next_token() {
            match token {
                Token::LineBreak => {
                    self.finish_line(VisitorContext::Tests, line_context);
                    line_context = LineContext::default();
                }
                Token::EndOfStatement => {
                    line_context.has_code = true;
                    self.skip_line(VisitorContext::Tests, line_context);
                    return;
                }
                Token::CodeBlockOpen => {
                    self.visit_code_block(VisitorContext::Tests, line_context, false);
                    line_context.has_code = true;
                    self.skip_line(VisitorContext::Tests, line_context);
                    return;
                }
                Token::WhiteSpace => {}
                _ => {
                    if !line_context.is_inside_comment() {
                        line_context.has_code = true;
                    }
                }
            }
        }
    }

    fn skip_line(&mut self, context: VisitorContext, line_context: LineContext) {
        while let Some(char) = self.next_char() {
            if char == '\n' {
                break;
            }
        }

        self.finish_line(context, line_context);
    }

    fn visit_code_block(
        &mut self,
        context: VisitorContext,
        line_context: LineContext,
        till_the_end: bool,
    ) {
        let mut line_context = line_context;
        while let Some(token) = self.next_token() {
            match token {
                Token::LineBreak => {
                    self.finish_line(context, line_context);
                    line_context = LineContext::default();
                }
                Token::WhiteSpace => {}
                Token::CommentStart => {
                    line_context.has_comment_start = true;
                    self.skip_line(context, line_context);
                    line_context = LineContext::default();
                }
                Token::DocCommentStart => {
                    line_context.has_doc_comment_start = true;
                    self.skip_line(context, line_context);
                    line_context = LineContext::default();
                }
                Token::CommentBlockOpen => {
                    self.visit_comment_block(context, false);
                    line_context.has_comment_start = true;
                }
                Token::DocCommentBlockOpen => {
                    self.visit_comment_block(context, true);
                    line_context.has_doc_comment_start = true;
                }
                Token::TestBlockStart => {
                    self.visit_test_block();
                }
                Token::CodeBlockOpen => {
                    self.visit_code_block(context, line_context, false);
                    line_context.has_code = true;
                }
                Token::CodeBlockClose => {
                    if !till_the_end {
                        return;
                    }
                }
                Token::DoubleQuote => {
                    self.visit_string(context);
                    line_context.has_code = true;
                }
                Token::StringBlockOpen => {
                    self.visit_string_block(context, Token::StringBlockClose);
                    line_context.has_code = true;
                }
                Token::DoubleStringBlockOpen => {
                    self.visit_string_block(context, Token::DoubleStringBlockClose);
                    line_context.has_code = true;
                }
                _ => line_context.has_code = true,
            }
        }
    }

    fn visit_string_block(&mut self, context: VisitorContext, closing_token: Token) {
        let mut line_context = LineContext {
            has_code: true,
            has_comment_start: false,
            has_doc_comment_start: false,
        };

        while let Some(token) = self.next_token() {
            match token {
                Token::LineBreak => {
                    self.finish_line(context, line_context);
                    line_context = LineContext::default();
                }
                v if v == closing_token => {
                    return;
                }
                _ => line_context.has_code = true,
            }
        }
    }

    fn visit_string(&mut self, context: VisitorContext) {
        let mut line_context = LineContext {
            has_code: true,
            has_comment_start: false,
            has_doc_comment_start: false,
        };

        while let Some(token) = self.next_token() {
            match token {
                Token::LineBreak => {
                    self.finish_line(context, line_context);
                    line_context = LineContext::default();
                }
                Token::DoubleQuote => return,
                _ => line_context.has_code = true,
            }
        }
    }

    fn visit_comment_block(&mut self, context: VisitorContext, is_doc: bool) {
        let mut line_context = LineContext {
            has_code: false,
            has_comment_start: !is_doc,
            has_doc_comment_start: is_doc,
        };

        while let Some(token) = self.next_token() {
            match token {
                Token::LineBreak => {
                    self.finish_line(context, line_context);
                    line_context = LineContext::default();
                }
                Token::CommentBlockOpen => {
                    self.visit_comment_block(context, false);
                }
                Token::CommentBlockClose => {
                    return;
                }
                Token::DocCommentBlockOpen => {
                    self.visit_comment_block(context, true);
                }
                Token::WhiteSpace => {}
                _ => {
                    line_context.has_comment_start = !is_doc;
                    line_context.has_doc_comment_start = is_doc;
                }
            }
        }
    }

    /// Finish processing a line and categorize it.
    ///
    /// The key insight: only logic lines need context (code/tests/examples).
    /// Comments, docs, and blanks are counted regardless of where they appear.
    fn finish_line(&mut self, context: VisitorContext, line_context: LineContext) {
        let curr = std::mem::take(&mut self.curr_string);
        let line = self.curr_line_no;
        self.curr_line_no += 1;

        // Always increment all (total line count)
        self.stats.all += 1;

        if line_context.has_code {
            // Logic lines depend on context
            match context {
                VisitorContext::Code => self.stats.code += 1,
                VisitorContext::Tests => self.stats.tests += 1,
                VisitorContext::Example => self.stats.examples += 1,
            }
            if self.debug {
                let ctx = match context {
                    VisitorContext::Code => "CODE",
                    VisitorContext::Tests => "TEST",
                    VisitorContext::Example => "EXAMPLE",
                };
                eprint!("{line}: {ctx}: {curr}");
            }
        } else if line_context.has_doc_comment_start {
            // Doc comments are context-independent
            self.stats.docs += 1;
            if self.debug {
                eprint!("{line}: DOCS: {curr}");
            }
        } else if line_context.has_comment_start {
            // Regular comments are context-independent
            self.stats.comments += 1;
            if self.debug {
                eprint!("{line}: COMM: {curr}");
            }
        } else {
            // Blank lines are context-independent
            self.stats.blanks += 1;
            if self.debug {
                eprint!("{line}: BLANK: {curr}");
            }
        }
    }

    fn next_token(&mut self) -> Option<Token> {
        let next_char = self.next_char()?;
        let token = match next_char {
            '\n' => Token::LineBreak,
            '/' if self.lookahead == Some('/') => {
                let _ = self.next_char();
                if self.lookahead == Some('/') || self.lookahead == Some('!') {
                    let next_char = self.next_char()?;
                    if next_char == '/' && self.lookahead == Some('/') {
                        Token::CommentStart
                    } else {
                        Token::DocCommentStart
                    }
                } else {
                    Token::CommentStart
                }
            }
            '/' if self.lookahead == Some('*') => {
                let mut string = '/'.to_string();
                self.collect_while(&mut string, |c| c == '!' || c == '*' || c == '/');
                match string.as_str() {
                    "/**" | "/*!" => Token::DocCommentBlockOpen,
                    v if v.ends_with("*/") => Token::WhiteSpace,
                    _ => Token::CommentBlockOpen,
                }
            }
            '*' if self.lookahead == Some('/') => {
                let _ = self.next_char();
                Token::CommentBlockClose
            }
            '#' if self.lookahead == Some('[') => {
                let mut string = '#'.to_string();
                self.collect_while(&mut string, |c| c != ']' && c != '\n');

                if let Some(next) = self.lookahead {
                    match next {
                        ']' => {
                            let _ = self.next_char();
                            string.push(next)
                        }
                        _ => return Some(Token::Other),
                    }
                }

                match string.as_str() {
                    "#[cfg(test)]" | "#[test]" => Token::TestBlockStart,
                    _ => Token::Other,
                }
            }
            '{' => Token::CodeBlockOpen,
            '}' => Token::CodeBlockClose,
            ';' => Token::EndOfStatement,
            '\\' if self.lookahead == Some('\\') => {
                let _ = self.next_char();
                Token::DoubleBackSlash
            }
            '\\' if self.lookahead == Some('"') => {
                let _ = self.next_char();
                Token::EscapedDoubleQuote
            }
            '"' if self.lookahead == Some('#') => {
                let mut string = '"'.to_string();
                self.collect_while(&mut string, |c| c == '#');
                match string.as_ref() {
                    "\"#" => Token::StringBlockClose,
                    "\"##" => Token::DoubleStringBlockClose,
                    _ => Token::Other,
                }
            }
            '"' => Token::DoubleQuote,
            'r' if self.lookahead == Some('#') => {
                let mut string = 'r'.to_string();
                self.collect_while(&mut string, |c| c == '#' || c == '"');
                match string.as_ref() {
                    "r#\"" => Token::StringBlockOpen,
                    "r##\"" => Token::DoubleStringBlockOpen,
                    _ => Token::Other,
                }
            }
            v if v.is_whitespace() => Token::WhiteSpace,
            _ => Token::Other,
        };

        Some(token)
    }

    fn collect_while(&mut self, string: &mut String, mut predicate: impl FnMut(char) -> bool) {
        while let Some(next_char) = self.lookahead {
            if predicate(next_char) {
                let _ = self.next_char();
                string.push(next_char);
            } else {
                break;
            }
        }
    }

    fn next_char(&mut self) -> Option<char> {
        let c = mem::replace(
            &mut self.lookahead,
            self.reader.chars().next().and_then(|c| c.ok()),
        );

        if let Some(c) = c {
            self.curr_string.push(c);
        }
        c
    }
}

/// Gather LOC statistics for a file at the given path.
///
/// This is the primary entry point for analyzing a single file.
/// The context (code/tests/examples) is automatically determined from the path.
///
/// # Example
///
/// ```rust
/// use rustloclib::gather_stats_for_path;
/// use std::fs;
/// use tempfile::tempdir;
///
/// let dir = tempdir().unwrap();
/// let file_path = dir.path().join("main.rs");
/// fs::write(&file_path, "fn main() {\n    println!(\"Hello\");\n}\n").unwrap();
///
/// let stats = gather_stats_for_path(&file_path).unwrap();
/// assert_eq!(stats.code, 3);
/// ```
pub fn gather_stats_for_path(path: impl AsRef<Path>) -> Result<Locs> {
    let visitor = Visitor::new(path, false)?;
    Ok(visitor.visit_file())
}

/// Gather LOC statistics from a string of Rust source code.
///
/// The context determines how logic lines are categorized:
/// - `VisitorContext::Code` → logic lines count as `code`
/// - `VisitorContext::Tests` → logic lines count as `tests`
/// - `VisitorContext::Example` → logic lines count as `examples`
///
/// Comments, docs, and blanks are always counted regardless of context.
///
/// # Example
///
/// ```rust
/// use rustloclib::{gather_stats, VisitorContext};
///
/// let source = r#"
/// fn main() {
///     println!("Hello");
/// }
/// "#;
///
/// let stats = gather_stats(source, VisitorContext::Code);
/// assert_eq!(stats.code, 3);
/// ```
pub fn gather_stats(source: &str, context: VisitorContext) -> Locs {
    let visitor = Visitor::from_reader(source.as_bytes(), context, false);
    visitor.visit_file()
}

// Keep old names as aliases for backwards compatibility during transition
#[doc(hidden)]
pub use gather_stats as parse_string;
#[doc(hidden)]
pub use gather_stats_for_path as parse_file;

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(file: &str) -> Locs {
        gather_stats(file, VisitorContext::Code)
    }

    #[test]
    fn empty_file() {
        let file = "\n";
        let stats = stats(file);

        assert_eq!(stats.blanks, 1);
        assert_eq!(stats.total(), 1);
    }

    #[test]
    fn one_empty_string() {
        let file = "  \t\t \n";
        let stats = stats(file);

        assert_eq!(stats.blanks, 1);
        assert_eq!(stats.total(), 1);
    }

    #[test]
    fn one_code_string() {
        let file = "mod lib;\n";
        let stats = stats(file);

        assert_eq!(stats.code, 1);
        assert_eq!(stats.total(), 1);
    }

    #[test]
    fn single_comment() {
        let file = "   // Comment\n";
        let stats = stats(file);

        assert_eq!(stats.comments, 1);
        assert_eq!(stats.total(), 1);
    }

    #[test]
    fn single_doc() {
        let file = "   /// Documentation\n";
        let stats = stats(file);

        assert_eq!(stats.docs, 1);
        assert_eq!(stats.total(), 1);
    }

    #[test]
    fn single_module_doc() {
        let file = "   //! Documentation\n";
        let stats = stats(file);

        assert_eq!(stats.docs, 1);
        assert_eq!(stats.total(), 1);
    }

    #[test]
    fn comment_block() {
        let file = "   /* comment */ \n";
        let stats = stats(file);

        assert_eq!(stats.comments, 1);
        assert_eq!(stats.total(), 1);
    }

    #[test]
    fn multiline_comment_block() {
        let file = r#"   /*

        comment
        */
"#;

        let stats = stats(file);

        assert_eq!(stats.comments, 3);
        assert_eq!(stats.blanks, 1);
        assert_eq!(stats.total(), 4);
    }

    #[test]
    fn doc_comment_block() {
        let file = "   /** comment */ \n";
        let stats = stats(file);

        assert_eq!(stats.docs, 1);
        assert_eq!(stats.total(), 1);
    }

    #[test]
    fn multiline_doc_comment_block() {
        let file = r#"   /*!

        comment
        */
"#;

        let stats = stats(file);

        assert_eq!(stats.docs, 3);
        assert_eq!(stats.blanks, 1);
        assert_eq!(stats.total(), 4);
    }

    #[test]
    fn comment_in_string_literals() {
        let file = r#"
let string = "Not a comment /*";
let a = 1;
"#;

        let stats = stats(file);

        assert_eq!(stats.comments, 0);
        assert_eq!(stats.code, 2);
    }

    #[test]
    fn test_block() {
        let file = r#"
#[cfg(test)]
mod tests {

    use super::*;

}
"#;

        let stats = stats(file);

        // Test logic lines
        assert_eq!(stats.tests, 4);
        // Blanks (context-independent)
        assert_eq!(stats.blanks, 3); // first line + 2 inside
        assert_eq!(stats.total(), 7);
    }

    #[test]
    fn test_attribute_function() {
        let file = r#"
#[test]
fn my_test() {
    assert!(true);
}
"#;

        let stats = stats(file);

        // First empty line is blank
        assert_eq!(stats.blanks, 1);
        // #[test], fn, assert, } are all test code
        assert_eq!(stats.tests, 4);
    }

    #[test]
    fn multiline_string_literals() {
        let file = r##"
let string = r#"

This is a string
// This is also a string

"#;

"##;

        let stats = stats(file);

        assert_eq!(stats.code, 4);
        assert_eq!(stats.blanks, 4);
        assert_eq!(stats.total(), 8);
    }

    #[test]
    fn mixed_code_and_tests() {
        let file = r#"
fn production_code() {
    println!("Hello");
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_something() {
        assert!(true);
    }
}
"#;

        let stats = stats(file);

        // Production code: fn + println + }
        assert_eq!(stats.code, 3);
        // Test code: #[cfg(test)] + mod tests { + #[test] + fn test + assert + } + }
        assert_eq!(stats.tests, 7);
        // Blanks: first line + line after } (context-independent)
        assert_eq!(stats.blanks, 2);
    }

    #[test]
    fn context_from_path() {
        assert_eq!(
            VisitorContext::from_file_path("src/lib.rs"),
            VisitorContext::Code
        );
        assert_eq!(
            VisitorContext::from_file_path("tests/integration.rs"),
            VisitorContext::Tests
        );
        assert_eq!(
            VisitorContext::from_file_path("examples/demo.rs"),
            VisitorContext::Example
        );
        assert_eq!(
            VisitorContext::from_file_path("src/tests.rs"),
            VisitorContext::Tests
        );
    }

    #[test]
    fn nested_comments() {
        let file = r#"
/* outer /* nested */ still outer */
fn code() {}
"#;

        let stats = stats(file);

        assert_eq!(stats.comments, 1);
        assert_eq!(stats.code, 1);
    }

    #[test]
    fn doc_and_regular_comments() {
        let file = r#"
/// This is a doc comment
// This is a regular comment
fn documented() {}
"#;

        let stats = stats(file);

        assert_eq!(stats.docs, 1);
        assert_eq!(stats.comments, 1);
        assert_eq!(stats.code, 1);
    }

    #[test]
    fn code_after_comment_on_same_line() {
        // If there's code before a comment, it's a code line
        let file = "let x = 1; // inline comment\n";
        let stats = stats(file);

        // This is counted as code because there's code before the comment
        assert_eq!(stats.code, 1);
        assert_eq!(stats.comments, 0);
    }

    #[test]
    fn empty_lines_between_code() {
        let file = r#"
fn foo() {

    let x = 1;

}
"#;

        let stats = stats(file);

        assert_eq!(stats.code, 3); // fn, let, }
        assert_eq!(stats.blanks, 3); // empty line at start, two empty lines inside
    }

    #[test]
    fn examples_context() {
        let source = "fn example() {}\n";
        let stats = gather_stats(source, VisitorContext::Example);

        assert_eq!(stats.examples, 1);
        assert_eq!(stats.code, 0);
        assert_eq!(stats.tests, 0);
    }

    #[test]
    fn comments_and_blanks_are_context_independent() {
        // Comments and blanks should be counted the same regardless of context
        let source = "// comment\n\n";

        let code_stats = gather_stats(source, VisitorContext::Code);
        let test_stats = gather_stats(source, VisitorContext::Tests);
        let example_stats = gather_stats(source, VisitorContext::Example);

        // All should have same comments and blanks
        assert_eq!(code_stats.comments, 1);
        assert_eq!(test_stats.comments, 1);
        assert_eq!(example_stats.comments, 1);

        assert_eq!(code_stats.blanks, 1);
        assert_eq!(test_stats.blanks, 1);
        assert_eq!(example_stats.blanks, 1);
    }
}
