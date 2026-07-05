use std::path::Path;

use oxc_allocator::Allocator;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

use crate::Result;

use super::backend::{
    generic_context_from_path, FileAnalysis, LanguageBackend, LanguageId, LineClass, LogicContext,
};
use super::stats::Locs;

/// TypeScript backend using Oxc comment spans for parser-backed classification.
#[derive(Debug, Default)]
pub struct TypeScriptBackend;

impl LanguageBackend for TypeScriptBackend {
    fn supports_path(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("ts") || ext.eq_ignore_ascii_case("tsx"))
    }

    fn analyze_source(&self, path: &Path, source: &str) -> Result<FileAnalysis> {
        let context = generic_context_from_path(path);
        let line_classes = classify_typescript_lines(path, source, context);

        let mut stats = Locs::new();
        for class in &line_classes {
            class.record(&mut stats);
        }

        Ok(FileAnalysis {
            language: LanguageId::TypeScript,
            stats,
            line_classes,
        })
    }
}

fn classify_typescript_lines(path: &Path, source: &str, context: LogicContext) -> Vec<LineClass> {
    let line_starts = line_starts(source);
    let mut line_classes: Vec<LineClass> = source
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                LineClass::Blanks
            } else {
                LineClass::Logic(context)
            }
        })
        .collect();

    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_else(|_| {
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("tsx"))
        {
            SourceType::tsx()
        } else {
            SourceType::ts()
        }
    });
    let parsed = Parser::new(&allocator, source, source_type)
        .with_options(ParseOptions {
            parse_regular_expression: true,
            ..ParseOptions::default()
        })
        .parse();

    for comment in &parsed.program.comments {
        let span = comment.span;
        let start = span.start as usize;
        let end = span.end as usize;
        let comment_text = &source[start.min(source.len())..end.min(source.len())];
        let class = if is_typescript_doc_comment(comment_text) {
            LineClass::Docs
        } else {
            LineClass::Comments
        };
        mark_full_comment_lines(source, &line_starts, &mut line_classes, start, end, class);
    }

    line_classes
}

fn is_typescript_doc_comment(comment: &str) -> bool {
    let trimmed = comment.trim_start();
    trimmed.starts_with("/**")
        || trimmed.starts_with("/// <reference")
        || trimmed.starts_with("/// <amd")
}

fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' && index + 1 < source.len() {
            starts.push(index + 1);
        }
    }
    starts
}

fn mark_full_comment_lines(
    source: &str,
    line_starts: &[usize],
    line_classes: &mut [LineClass],
    start: usize,
    end: usize,
    class: LineClass,
) {
    if line_classes.is_empty() || start >= source.len() {
        return;
    }

    let first_line = line_index(line_starts, start);
    let last_byte = end.saturating_sub(1).min(source.len().saturating_sub(1));
    let last_line = line_index(line_starts, last_byte).min(line_classes.len() - 1);

    for (line_index, line_class) in line_classes
        .iter_mut()
        .enumerate()
        .take(last_line + 1)
        .skip(first_line)
    {
        if is_full_line_comment(source, line_starts, line_index, start, end) {
            *line_class = class;
        }
    }
}

fn is_full_line_comment(
    source: &str,
    line_starts: &[usize],
    line_index: usize,
    comment_start: usize,
    comment_end: usize,
) -> bool {
    let line_start = line_starts[line_index];
    let line_end = line_starts
        .get(line_index + 1)
        .copied()
        .unwrap_or(source.len());
    let content_end = source[..line_end].trim_end_matches(['\r', '\n']).len();
    let comment_line_start = comment_start.max(line_start);
    let comment_line_end = comment_end.min(content_end);

    source[line_start..comment_line_start].trim().is_empty()
        && source[comment_line_end..content_end].trim().is_empty()
}

fn line_index(line_starts: &[usize], byte_index: usize) -> usize {
    match line_starts.binary_search(&byte_index) {
        Ok(index) => index,
        Err(index) => index.saturating_sub(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze(path: &str, source: &str) -> FileAnalysis {
        TypeScriptBackend
            .analyze_source(Path::new(path), source)
            .unwrap()
    }

    #[test]
    fn classifies_typescript_docs_comments_blanks_and_logic() {
        let analysis = analyze(
            "src/app.ts",
            r#"/** Public API. */
// implementation note

export function run(): boolean {
  return true;
}
"#,
        );

        assert_eq!(analysis.language, LanguageId::TypeScript);
        assert_eq!(analysis.stats.docs, 1);
        assert_eq!(analysis.stats.comments, 1);
        assert_eq!(analysis.stats.blanks, 1);
        assert_eq!(analysis.stats.code, 3);
    }

    #[test]
    fn classifies_multiline_jsdoc_and_ordinary_block_comments() {
        let analysis = analyze(
            "src/app.ts",
            r#"/**
 * Public API.
 */
/*
 * Internal note.
 */
export interface Service {
  run(): void;
}
"#,
        );

        assert_eq!(analysis.stats.docs, 3);
        assert_eq!(analysis.stats.comments, 3);
        assert_eq!(analysis.stats.code, 3);
        assert_eq!(analysis.stats.blanks, 0);
    }

    #[test]
    fn classifies_triple_slash_reference_and_amd_as_docs() {
        let analysis = analyze(
            "src/app.ts",
            r#"/// <reference types="node" />
/// <amd-module name="pkg/app" />
// regular comment
export const value = 1;
"#,
        );

        assert_eq!(analysis.stats.docs, 2);
        assert_eq!(analysis.stats.comments, 1);
        assert_eq!(analysis.stats.code, 1);
    }

    #[test]
    fn ignores_comment_markers_inside_strings() {
        let analysis = analyze(
            "src/app.ts",
            r#"const path = "not // a comment";
const block = "not /* a comment */";
"#,
        );

        assert_eq!(analysis.stats.comments, 0);
        assert_eq!(analysis.stats.code, 2);
    }

    #[test]
    fn ignores_comment_markers_inside_regexes_and_templates() {
        let analysis = analyze(
            "src/app.ts",
            r#"const slash = /\/\/ not a comment/;
const block = `not /* a comment */ and not // a comment`;
const url = "https://example.invalid/path";
"#,
        );

        assert_eq!(analysis.stats.docs, 0);
        assert_eq!(analysis.stats.comments, 0);
        assert_eq!(analysis.stats.code, 3);
    }

    #[test]
    fn keeps_inline_comments_as_logic_but_counts_full_line_comments() {
        let analysis = analyze(
            "src/app.ts",
            r#"const first = 1; // trailing comment
// full line comment
/* full line block */
const second = 2; /* trailing block */
"#,
        );

        assert_eq!(analysis.stats.code, 2);
        assert_eq!(analysis.stats.comments, 2);
        assert_eq!(analysis.stats.docs, 0);
    }

    #[test]
    fn classifies_tsx_and_decorator_shapes_as_logic() {
        let analysis = analyze(
            "src/components/widget.tsx",
            r#"@sealed
export class Widget {
  render() {
    return <section>{this.label}</section>;
  }
}

export interface Props {
  label: string;
}
"#,
        );

        assert_eq!(analysis.stats.code, 9);
        assert_eq!(analysis.stats.blanks, 1);
        assert_eq!(analysis.stats.docs, 0);
        assert_eq!(analysis.stats.comments, 0);
    }

    #[test]
    fn keeps_comments_from_parse_error_recovery() {
        let analysis = analyze(
            "src/bad.ts",
            r#"/** Recovered docs. */
// recovered comment
export const value = ;
"#,
        );

        assert_eq!(analysis.stats.docs, 1);
        assert_eq!(analysis.stats.comments, 1);
        assert_eq!(analysis.stats.code, 1);
    }

    #[test]
    fn path_level_test_files_count_logic_as_tests() {
        let analysis = analyze(
            "src/widget.test.tsx",
            r#"import { expect, test } from "vitest";

test("works", () => {
  expect(true).toBe(true);
});
"#,
        );

        assert_eq!(analysis.stats.tests, 4);
        assert_eq!(analysis.stats.code, 0);
        assert_eq!(analysis.stats.blanks, 1);
    }
}
