use std::process::Command;
use std::str::from_utf8;

#[test]
fn test_merge() {
    // Setup: Create temporary test files
    let source_file = "tests/horegopos_uploader_h5_202408191055.xlsx";
    let ref_file = "tests/restaurant_uploader_h5_202408160639_2.3.xlsx";
    let column = "印度尼西亚语";

    // Run the main program with test arguments
    let output = Command::new("cargo")
        .arg("run")
        .arg("--")
        .arg(source_file)
        .arg(ref_file)
        .arg(column)
        .output()
        .expect("Failed to execute process");

    println!("{}", from_utf8(&output.stderr).expect("Failed to convert stderr to string"));
    println!("{}", from_utf8(&output.stdout).expect("Failed to convert stdout to string"));
    // Check if the program executed successfully
    assert!(output.status.success());
}
