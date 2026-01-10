//! Compare rustloclib results with warloc output.

use rustloclib::{count_workspace, CountOptions};
use std::env;

fn main() {
    let path = env::args().nth(1).unwrap_or_else(|| ".".to_string());

    let result = count_workspace(&path, CountOptions::new()).expect("Failed to count workspace");

    println!("File count: {}", result.total.file_count);
    println!("Context      | Logic        | Blank        | Docs         | Comments     | Total");
    println!(
        "-------------|--------------|--------------|--------------|--------------|-------------"
    );
    println!(
        "Code         | {:12} | {:12} | {:12} | {:12} | {:12}",
        result.total.code.logic,
        result.total.code.blank,
        result.total.code.docs,
        result.total.code.comments,
        result.total.code.total()
    );
    println!(
        "Tests        | {:12} | {:12} | {:12} | {:12} | {:12}",
        result.total.tests.logic,
        result.total.tests.blank,
        result.total.tests.docs,
        result.total.tests.comments,
        result.total.tests.total()
    );
    println!(
        "Examples     | {:12} | {:12} | {:12} | {:12} | {:12}",
        result.total.examples.logic,
        result.total.examples.blank,
        result.total.examples.docs,
        result.total.examples.comments,
        result.total.examples.total()
    );
    println!(
        "-------------|--------------|--------------|--------------|--------------|-------------"
    );
    println!(
        "             | {:12} | {:12} | {:12} | {:12} | {:12}",
        result.total.logic(),
        result.total.blank(),
        result.total.docs(),
        result.total.comments(),
        result.total.total()
    );
}
