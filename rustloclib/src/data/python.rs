use std::path::Path;

use ruff_python_ast::{Arguments, ExceptHandler, Expr, Stmt, Suite};
use ruff_python_parser::parse_module;
use ruff_text_size::{Ranged, TextRange};

use crate::Result;

use super::backend::{FileAnalysis, LanguageBackend, LanguageId, LineClass, LogicContext};
use super::stats::Locs;

/// Python backend using Ruff syntax ranges for same-file semantic classification.
#[derive(Debug, Default)]
pub struct PythonBackend;

impl LanguageBackend for PythonBackend {
    fn supports_path(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("py") || ext.eq_ignore_ascii_case("pyw"))
    }

    fn analyze_source(&self, path: &Path, source: &str) -> Result<FileAnalysis> {
        let default_context = python_context_from_path(path);
        let mut line_classes = classify_python_lines(source, default_context);

        if let Ok(parsed) = parse_module(source) {
            let mut classifier = PythonSemanticClassifier::new(source, &mut line_classes);
            classifier.visit_suite(parsed.suite(), default_context);
        }

        let mut stats = Locs::new();
        for class in line_classes {
            class.record(&mut stats);
        }

        Ok(FileAnalysis {
            language: LanguageId::Python,
            stats,
        })
    }
}

fn python_context_from_path(path: &Path) -> LogicContext {
    let mut saw_example_dir = false;
    for component in path.components() {
        let Some(value) = component.as_os_str().to_str() else {
            continue;
        };
        let value = value.to_ascii_lowercase();
        match value.as_str() {
            "tests" | "test" => return LogicContext::Tests,
            "examples" | "example" | "samples" | "sample" => saw_example_dir = true,
            _ => {}
        }
    }

    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if stem.starts_with("test_") || stem.ends_with("_test") || filename.contains(".test.") {
        LogicContext::Tests
    } else if saw_example_dir || stem.starts_with("example_") || stem.ends_with("_example") {
        LogicContext::Example
    } else {
        LogicContext::Code
    }
}

fn classify_python_lines(source: &str, context: LogicContext) -> Vec<LineClass> {
    source
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                LineClass::Blanks
            } else if trimmed.starts_with('#') {
                LineClass::Comments
            } else {
                LineClass::Logic(context)
            }
        })
        .collect()
}

struct PythonSemanticClassifier<'a> {
    line_starts: Vec<usize>,
    line_classes: &'a mut [LineClass],
}

impl<'a> PythonSemanticClassifier<'a> {
    fn new(source: &str, line_classes: &'a mut [LineClass]) -> Self {
        let mut line_starts = vec![0];
        for (index, byte) in source.bytes().enumerate() {
            if byte == b'\n' && index + 1 < source.len() {
                line_starts.push(index + 1);
            }
        }

        Self {
            line_starts,
            line_classes,
        }
    }

    fn visit_suite(&mut self, suite: &Suite, context: LogicContext) {
        if let Some(docstring) = suite.first().filter(|stmt| is_docstring_stmt(stmt)) {
            self.mark_range(docstring.range(), LineClass::Docs);
        }

        for statement in suite {
            self.visit_stmt(statement, context);
        }
    }

    fn visit_stmt(&mut self, statement: &Stmt, context: LogicContext) {
        match statement {
            Stmt::FunctionDef(function) => {
                let context = if context == LogicContext::Tests
                    || is_pytest_function(function.name.as_str())
                {
                    LogicContext::Tests
                } else {
                    context
                };
                if context == LogicContext::Tests {
                    self.mark_range(function.range(), LineClass::Logic(LogicContext::Tests));
                }
                self.visit_suite(&function.body, context);
            }
            Stmt::ClassDef(class) => {
                let context = if context == LogicContext::Tests
                    || is_pytest_class(class.name.as_str())
                    || is_unittest_class(class.arguments.as_deref())
                {
                    LogicContext::Tests
                } else {
                    context
                };
                if context == LogicContext::Tests {
                    self.mark_range(class.range(), LineClass::Logic(LogicContext::Tests));
                }
                self.visit_suite(&class.body, context);
            }
            Stmt::For(stmt) => {
                self.visit_suite(&stmt.body, context);
                self.visit_suite(&stmt.orelse, context);
            }
            Stmt::While(stmt) => {
                self.visit_suite(&stmt.body, context);
                self.visit_suite(&stmt.orelse, context);
            }
            Stmt::If(stmt) => {
                self.visit_suite(&stmt.body, context);
                for clause in &stmt.elif_else_clauses {
                    self.visit_suite(&clause.body, context);
                }
            }
            Stmt::With(stmt) => {
                self.visit_suite(&stmt.body, context);
            }
            Stmt::Match(stmt) => {
                for case in &stmt.cases {
                    self.visit_suite(&case.body, context);
                }
            }
            Stmt::Try(stmt) => {
                self.visit_suite(&stmt.body, context);
                for handler in &stmt.handlers {
                    let ExceptHandler::ExceptHandler(handler) = handler;
                    self.visit_suite(&handler.body, context);
                }
                self.visit_suite(&stmt.orelse, context);
                self.visit_suite(&stmt.finalbody, context);
            }
            _ => {}
        }
    }

    fn mark_range(&mut self, range: TextRange, class: LineClass) {
        if self.line_classes.is_empty() {
            return;
        }

        let start = range.start().to_usize();
        let end = range.end().to_usize().saturating_sub(1);
        let start_line = self.line_for_offset(start);
        let end_line = self.line_for_offset(end).min(self.line_classes.len() - 1);

        for line_class in &mut self.line_classes[start_line..=end_line] {
            if !matches!(line_class, LineClass::Blanks | LineClass::Comments) {
                *line_class = class;
            }
        }
    }

    fn line_for_offset(&self, offset: usize) -> usize {
        self.line_starts
            .partition_point(|line_start| *line_start <= offset)
            .saturating_sub(1)
    }
}

fn is_docstring_stmt(statement: &Stmt) -> bool {
    matches!(
        statement,
        Stmt::Expr(expr) if matches!(expr.value.as_ref(), Expr::StringLiteral(_))
    )
}

fn is_pytest_function(name: &str) -> bool {
    name.starts_with("test_")
}

fn is_pytest_class(name: &str) -> bool {
    name.starts_with("Test")
}

fn is_unittest_class(arguments: Option<&Arguments>) -> bool {
    let Some(arguments) = arguments else {
        return false;
    };

    arguments.args.iter().any(is_unittest_base)
}

fn is_unittest_base(expr: &Expr) -> bool {
    match expr {
        Expr::Name(name) => name.id.as_str() == "TestCase",
        Expr::Attribute(attribute) => {
            attribute.attr.as_str() == "TestCase"
                && matches!(
                    attribute.value.as_ref(),
                    Expr::Name(name) if name.id.as_str() == "unittest"
                )
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze(path: &str, source: &str) -> Locs {
        PythonBackend
            .analyze_source(Path::new(path), source)
            .unwrap()
            .stats
    }

    #[test]
    fn classifies_pytest_functions_in_production_files() {
        let stats = analyze(
            "src/service.py",
            r#""""Module docs."""

def build():
    return 1

def test_build():
    assert build() == 1
"#,
        );

        assert_eq!(stats.docs, 1);
        assert_eq!(stats.blanks, 2);
        assert_eq!(stats.code, 2);
        assert_eq!(stats.tests, 2);
    }

    #[test]
    fn classifies_unittest_classes_in_production_files() {
        let stats = analyze(
            "src/service.py",
            r#"import unittest

class ServiceTest(unittest.TestCase):
    """Class docs."""
    def test_build(self):
        self.assertTrue(True)
"#,
        );

        assert_eq!(stats.code, 1);
        assert_eq!(stats.blanks, 1);
        assert_eq!(stats.docs, 1);
        assert_eq!(stats.tests, 3);
    }

    #[test]
    fn falls_back_to_path_level_counting_on_parse_errors() {
        let stats = analyze(
            "tests/test_bad.py",
            r#"# comment

def test_bad(
    assert True
"#,
        );

        assert_eq!(stats.comments, 1);
        assert_eq!(stats.blanks, 1);
        assert_eq!(stats.tests, 2);
    }

    #[test]
    fn classifies_example_paths() {
        let stats = analyze(
            "examples/demo.py",
            r#"def demo():
    return True
"#,
        );

        assert_eq!(stats.examples, 2);
    }
}
