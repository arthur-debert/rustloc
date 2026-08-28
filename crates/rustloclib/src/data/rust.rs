//! Rust source classification backed by rust-analyzer syntax trees.
//!
//! The classifier uses `ra_ap_syntax` for lossless token ranges and Rust item
//! attributes. It does not expand macros or expose parser diagnostics through
//! Rustloc's public result types.

use std::ops::Range;

use ra_ap_syntax::{
    ast::{self, HasAttrs},
    AstNode, AstToken, Edition, NodeOrToken, SourceFile, SyntaxKind, SyntaxNode, TextRange,
};

use super::backend::{FileAnalysis, LanguageId, LineClass, LogicContext};
use super::stats::Locs;

pub(crate) fn analyze_rust_source(source: &str, context: LogicContext) -> FileAnalysis {
    let parse = parse_best_effort(source);
    let root = parse.syntax_node();
    let line_ranges = line_ranges(source);
    let mut line_classes = vec![LineClass::Blanks; line_ranges.len()];
    let mut classifier = RustLineClassifier {
        source,
        line_ranges,
        line_classes: &mut line_classes,
    };

    classifier.classify_tokens(&root, context);
    classifier.mark_test_items(&root);

    let mut stats = Locs::new();
    for class in &line_classes {
        class.record(&mut stats);
    }

    FileAnalysis {
        language: LanguageId::Rust,
        stats,
        line_classes,
    }
}

fn parse_best_effort(source: &str) -> ra_ap_syntax::Parse<SourceFile> {
    let editions = [
        Edition::Edition2024,
        Edition::Edition2021,
        Edition::Edition2018,
        Edition::Edition2015,
    ];

    let mut best_parse = None;
    for edition in editions {
        let parse = SourceFile::parse(source, edition);
        let error_count = parse.errors().len();
        if error_count == 0 {
            return parse;
        }

        let should_replace = match &best_parse {
            Some((_parse, best_error_count)) => error_count < *best_error_count,
            None => true,
        };
        if should_replace {
            best_parse = Some((parse, error_count));
        }
    }

    best_parse
        .map(|(parse, _error_count)| parse)
        .expect("at least one Rust edition is available")
}

fn line_ranges(source: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut line_start = 0;

    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            ranges.push(line_start..index);
            line_start = index + 1;
        }
    }

    if line_start < source.len() {
        ranges.push(line_start..source.len());
    }

    ranges
}

struct RustLineClassifier<'a> {
    source: &'a str,
    line_ranges: Vec<Range<usize>>,
    line_classes: &'a mut [LineClass],
}

impl RustLineClassifier<'_> {
    fn classify_tokens(&mut self, root: &SyntaxNode, context: LogicContext) {
        for token in root
            .descendants_with_tokens()
            .filter_map(NodeOrToken::into_token)
        {
            match token.kind() {
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => {}
                SyntaxKind::COMMENT => {
                    let Some(comment) = ast::Comment::cast(token.clone()) else {
                        continue;
                    };
                    let class = if comment.is_doc() {
                        LineClass::Docs
                    } else {
                        LineClass::Comments
                    };
                    self.mark_non_logic_range(token.text_range(), class);
                }
                _ => self.mark_range(token.text_range(), LineClass::Logic(context), true),
            }
        }
    }

    fn mark_test_items(&mut self, root: &SyntaxNode) {
        for node in root.descendants() {
            if let Some(item) = ast::Item::cast(node.clone()) {
                if attrs_mark_test(item.attrs()) {
                    self.mark_range(
                        item.syntax().text_range(),
                        LineClass::Logic(LogicContext::Tests),
                        false,
                    );
                }
                continue;
            }

            if let Some(item) = ast::AssocItem::cast(node) {
                if attrs_mark_test(item.attrs()) {
                    self.mark_range(
                        item.syntax().text_range(),
                        LineClass::Logic(LogicContext::Tests),
                        false,
                    );
                }
            }
        }
    }

    fn mark_range(&mut self, range: TextRange, class: LineClass, overwrite_non_logic: bool) {
        let start = u32::from(range.start()) as usize;
        let end = u32::from(range.end()) as usize;

        for index in self.intersecting_line_indices(start, end) {
            let line_range = &self.line_ranges[index];
            let Some(slice) = intersect(line_range, start..end) else {
                continue;
            };

            if self.source[slice].trim().is_empty() {
                continue;
            }

            let existing = self.line_classes[index];
            if overwrite_non_logic || matches!(existing, LineClass::Logic(_)) {
                self.line_classes[index] = class;
            }
        }
    }

    fn mark_non_logic_range(&mut self, range: TextRange, class: LineClass) {
        let start = u32::from(range.start()) as usize;
        let end = u32::from(range.end()) as usize;

        for index in self.intersecting_line_indices(start, end) {
            let line_range = &self.line_ranges[index];
            let Some(slice) = intersect(line_range, start..end) else {
                continue;
            };

            if self.source[slice].trim().is_empty() {
                continue;
            }

            if self.line_classes[index] == LineClass::Blanks {
                self.line_classes[index] = class;
            }
        }
    }

    fn intersecting_line_indices(&self, start: usize, end: usize) -> Range<usize> {
        let first = self.line_ranges.partition_point(|line| line.end <= start);
        let last = self.line_ranges.partition_point(|line| line.start < end);
        first..last
    }
}

fn intersect(line: &Range<usize>, token: Range<usize>) -> Option<Range<usize>> {
    let start = line.start.max(token.start);
    let end = line.end.min(token.end);
    (start < end).then_some(start..end)
}

fn attrs_mark_test(mut attrs: impl Iterator<Item = ast::Attr>) -> bool {
    attrs.any(|attr| attr_marks_test(&attr))
}

fn attr_marks_test(attr: &ast::Attr) -> bool {
    match attr.meta() {
        Some(ast::Meta::PathMeta(meta)) => meta
            .path()
            .is_some_and(|path| path.syntax().text() == "test"),
        Some(ast::Meta::CfgMeta(meta)) => meta
            .cfg_predicate()
            .is_some_and(|predicate| cfg_requires_test(&predicate)),
        _ => false,
    }
}

fn cfg_requires_test(predicate: &ast::CfgPredicate) -> bool {
    match predicate {
        ast::CfgPredicate::CfgAtom(atom) => atom
            .ident_token()
            .is_some_and(|ident| ident.text() == "test" && atom.eq_token().is_none()),
        ast::CfgPredicate::CfgComposite(composite) => {
            let Some(keyword) = composite.keyword() else {
                return false;
            };
            match keyword.text() {
                "all" => composite
                    .cfg_predicates()
                    .any(|predicate| cfg_requires_test(&predicate)),
                "any" => {
                    let mut predicates = composite.cfg_predicates();
                    let Some(first) = predicates.next() else {
                        return false;
                    };
                    cfg_requires_test(&first)
                        && predicates.all(|predicate| cfg_requires_test(&predicate))
                }
                "not" => {
                    let mut predicates = composite.cfg_predicates();
                    let Some(predicate) = predicates.next() else {
                        return false;
                    };
                    predicates.next().is_none() && cfg_not_requires_test(&predicate)
                }
                _ => false,
            }
        }
    }
}

fn cfg_not_requires_test(predicate: &ast::CfgPredicate) -> bool {
    let ast::CfgPredicate::CfgComposite(composite) = predicate else {
        return false;
    };
    let Some(keyword) = composite.keyword() else {
        return false;
    };
    if keyword.text() != "not" {
        return false;
    }

    let mut predicates = composite.cfg_predicates();
    let Some(predicate) = predicates.next() else {
        return false;
    };
    predicates.next().is_none() && cfg_requires_test(&predicate)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(source: &str) -> Locs {
        analyze_rust_source(source, LogicContext::Code).stats
    }

    fn line_classes(source: &str) -> Vec<LineClass> {
        analyze_rust_source(source, LogicContext::Code).line_classes
    }

    #[test]
    fn ordinary_string_beginning_with_hash_does_not_hide_later_test() {
        let source = r##"
fn production() {
    let text = "#[not_an_attr]";
}

#[test]
fn still_a_test() {
    assert!(true);
}
"##;

        let stats = stats(source);

        assert_eq!(stats.code, 3);
        assert_eq!(stats.tests, 4);
    }

    #[test]
    fn quoted_braces_in_char_and_byte_char_literals_do_not_change_nesting() {
        let source = r#"
fn production() {
    let quote = '"';
    let byte_quote = b'"';
    let brace = '}';
}

#[test]
fn still_a_test() {
    assert_eq!('"', '"');
}
"#;

        let stats = stats(source);

        assert_eq!(stats.code, 5);
        assert_eq!(stats.tests, 4);
    }

    #[test]
    fn every_rust_literal_form_ignores_comment_like_text() {
        let source = r###"
fn literals() {
    let ordinary = "/* not a comment */";
    let byte = b"// not a comment";
    let c = c"/* not a comment */";
    let raw = r#"// not a comment"#;
    let raw_byte = br#"/* not a comment */"#;
    let raw_c = cr#"// not a comment"#;
}
"###;

        let stats = stats(source);

        assert_eq!(stats.code, 8);
        assert_eq!(stats.comments, 0);
    }

    #[test]
    fn raw_strings_with_zero_several_and_255_hashes_are_logic() {
        let many_hashes = "#".repeat(255);
        let source = format!(
            "\
fn raw() {{
    let zero = r\"// not a comment\";
    let several = r###\"/* not a comment */\"###;
    let max = r{hashes}\"// not a comment\"{hashes};
}}
",
            hashes = many_hashes
        );

        let stats = stats(&source);

        assert_eq!(stats.code, 5);
        assert_eq!(stats.comments, 0);
    }

    #[test]
    fn lifetimes_and_labels_are_logic_not_comments() {
        let source = r#"
fn labels<'a>(value: &'a str) -> &'a str {
    'outer: loop {
        break 'outer value;
    }
}
"#;

        let stats = stats(source);

        assert_eq!(stats.code, 5);
        assert_eq!(stats.comments, 0);
    }

    #[test]
    fn nested_block_comments_and_literal_text_are_classified_by_token() {
        let source = r#"
/* outer
   /* nested */
   still outer
*/
fn code() {
    let text = "/* not a comment */";
}
"#;

        let stats = stats(source);

        assert_eq!(stats.comments, 4);
        assert_eq!(stats.code, 3);
    }

    #[test]
    fn test_attributes_spaced_attributes_and_compound_cfg_are_tests() {
        let source = r#"
#[test]
fn direct() {}

# [ test ]
fn spaced() {}

#[cfg(test)]
fn cfg_test() {}

#[cfg(all(test, unix))]
fn cfg_all_test() {}

#[cfg(any(test, unix))]
fn cfg_any_test() {}

#[cfg(not(test))]
fn cfg_not_test() {}
"#;

        let stats = stats(source);

        assert_eq!(stats.tests, 8);
        assert_eq!(stats.code, 4);
    }

    #[test]
    fn cfg_double_negated_test_marks_item_as_tests() {
        let source = r#"
#[cfg(not(not(test)))]
fn double_negated() {}

#[cfg(not(not(all(test, unix))))]
fn double_negated_compound() {}

#[cfg(not(not(any(test, unix))))]
fn double_negated_any() {}
"#;

        let stats = stats(source);

        assert_eq!(stats.tests, 4);
        assert_eq!(stats.code, 2);
    }

    #[test]
    fn cfg_test_module_marks_nested_logic_as_tests() {
        let source = r#"
#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn nested() {
        assert!(true);
    }
}
"#;

        let stats = stats(source);

        assert_eq!(stats.tests, 8);
        assert_eq!(stats.blanks, 3);
        assert_eq!(stats.code, 0);
    }

    #[test]
    fn supplied_context_classifies_non_test_logic() {
        let source = "fn example() {}\n";
        let stats = analyze_rust_source(source, LogicContext::Example).stats;

        assert_eq!(stats.examples, 1);
        assert_eq!(stats.code, 0);
        assert_eq!(stats.tests, 0);
    }

    #[test]
    fn final_source_line_without_trailing_newline_is_counted() {
        let source = "#[test]\nfn final_line() {}";

        let classes = line_classes(source);
        let stats = stats(source);

        assert_eq!(classes.len(), 2);
        assert_eq!(stats.tests, 2);
        assert_eq!(stats.total(), 2);
    }

    #[test]
    fn incomplete_source_recovers_without_panicking() {
        let source = r#"
fn unfinished() {
    let text = "unterminated
"#;

        let analysis = analyze_rust_source(source, LogicContext::Code);

        assert_eq!(analysis.line_classes.len(), 3);
        assert_eq!(analysis.stats.total(), 3);
    }

    #[test]
    fn comments_docs_and_blanks_stay_context_independent_in_test_items() {
        let source = r#"
#[test]
fn documented_test() {
    /// local docs
    // local comment

    assert!(true); // inline comment stays logic
}
"#;

        let stats = stats(source);

        assert_eq!(stats.tests, 4);
        assert_eq!(stats.docs, 1);
        assert_eq!(stats.comments, 1);
        assert_eq!(stats.blanks, 2);
    }
}
