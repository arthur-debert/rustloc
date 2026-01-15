//! Compare rustloclib results with warloc output.

use rustloclib::{count_workspace, CountOptions};
use std::env;

fn main() {
    let path = env::args().nth(1).unwrap_or_else(|| ".".to_string());

    let result = count_workspace(&path, CountOptions::new()).expect("Failed to count workspace");

    println!("rustloc flat line type model");
    println!("=============================");
    println!();
    println!(
        "Line Type    | Count\n\
         -------------|-------------"
    );
    println!("Code         | {:12}", result.total.code);
    println!("Tests        | {:12}", result.total.tests);
    println!("Examples     | {:12}", result.total.examples);
    println!("Docs         | {:12}", result.total.docs);
    println!("Comments     | {:12}", result.total.comments);
    println!("Blanks       | {:12}", result.total.blanks);
    println!("-------------|-------------");
    println!("Total        | {:12}", result.total.total());
    println!();
    println!(
        "Logic lines (code + tests + examples): {}",
        result.total.total_logic()
    );
}
