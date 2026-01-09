//! Compare rustloclib results with warloc output.

use rustloclib::{count_workspace, CountOptions};
use std::env;

fn main() {
    let path = env::args().nth(1).unwrap_or_else(|| ".".to_string());

    let result = count_workspace(&path, CountOptions::new()).expect("Failed to count workspace");

    println!("File count: {}", result.total.file_count);
    println!("Type         | Code         | Blank        | Doc comments | Comments     | Total");
    println!(
        "-------------|--------------|--------------|--------------|--------------|-------------"
    );
    println!(
        "Main         | {:12} | {:12} | {:12} | {:12} | {:12}",
        result.total.main.code,
        result.total.main.blanks,
        result.total.main.docs,
        result.total.main.comments,
        result.total.main.total()
    );
    println!(
        "Tests        | {:12} | {:12} | {:12} | {:12} | {:12}",
        result.total.tests.code,
        result.total.tests.blanks,
        result.total.tests.docs,
        result.total.tests.comments,
        result.total.tests.total()
    );
    println!(
        "Examples     | {:12} | {:12} | {:12} | {:12} | {:12}",
        result.total.examples.code,
        result.total.examples.blanks,
        result.total.examples.docs,
        result.total.examples.comments,
        result.total.examples.total()
    );
    println!(
        "-------------|--------------|--------------|--------------|--------------|-------------"
    );
    println!(
        "             | {:12} | {:12} | {:12} | {:12} | {:12}",
        result.total.code(),
        result.total.blanks(),
        result.total.docs(),
        result.total.comments(),
        result.total.total()
    );
}
