use std::process::Command;

#[test]
fn test_merge() {
    // Setup: Create temporary test files
    let source_file = " ../../tests/horegopos_mobile_202408191053.xlsx ";
    let ref_file = "../../tests/restaurant_mobile_202408160626_2.3.xlsx";
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

    // Check if the program executed successfully
    assert!(output.status.success());
}
