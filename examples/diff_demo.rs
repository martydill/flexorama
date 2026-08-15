use std::io::{self, Write};

fn main() {
    // Simulate what the diff formatter produces
    let diff_output = r#"Changes in test_diff_sample.txt
@@ -1,3 +1,3 @@
fn main() {
-    println!("Testing");
+    println!("Modified");
}
"#;

    // Show what it looks like with colors
    println!("Simulated diff output for Edit tool:");
    println!("{}", diff_output);

    // Show what write diff looks like
    let write_output = r#"New file new_file.txt
20 bytes

Preview (first 10 lines):
+ line 1
+ line 2
+ line 3
"#;

    println!("\nSimulated diff output for Write tool:");
    println!("{}", write_output);

    // Flush to ensure all output is visible
    io::stdout().flush().unwrap();
}
