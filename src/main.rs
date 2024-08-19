use std::env;
use std::process::exit;

mod merge;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: {} <source_file> <ref_file> <column>", args[0]);
        exit(1);
    }

    let source_file = &args[1];
    let ref_file = &args[2];
    let column = &args[3];

    merge::merge(source_file, ref_file, column);
    exit(0)
}
