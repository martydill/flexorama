use colored::Colorize;

fn main() {
    // Simulate colored diff output
    println!("\n{}", "Example: Edit tool diff output".bold());
    println!("-------------------");
    println!("{} {}", "Changes in".cyan().bold(), "src/main.rs".white().bold());
    println!("{}", "@@ -1,3 +1,3 @@".dimmed());
    println!("{}", "fn main() {".red());
    println!("{}", "-     println!(\"Hello\");".red());
    println!("{}", "+     println!(\"Hello, World!\");".green());
    println!("{}", "}".red());

    println!("\n{}", "Example: Write tool diff output".bold());
    println!("--------------------");
    println!("{} {}", "New file".cyan().bold(), "src/new.rs".white().bold());
    println!("{}", "20 bytes".dimmed());
    println!();
    println!("{}", "Preview (first 10 lines):".dimmed());
    println!("{}", "+ line 1".green());
    println!("{}", "+ line 2".green());
    println!("{}", "+ line 3".green());

    println!("\n{}", "Color Legend:".bold());
    println!("  - {}", "- deleted lines".red());
    println!("  - {}", "+ added lines".green());
    println!("  - {}", "@@ hunk headers @@".dimmed());
    println!("  - {}", "filenames/headers".cyan().bold());
}
