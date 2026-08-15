use crate::tools::registry::DisplayContext;

/// Simple display for tool calls suitable for pipes/logs.
pub struct SimpleDisplay {
    context: DisplayContext,
}

impl SimpleDisplay {
    pub fn new(context: DisplayContext) -> Self {
        Self { context }
    }
}

impl super::ToolDisplay for SimpleDisplay {
    fn complete_success(&mut self, result: &str) {
        let duration = self.context.start_time.elapsed();
        // Show full result for Edit/Write tools to see diffs
        if self.context.tool_name == "Edit" || self.context.tool_name == "Write" {
            app_println!(
                "[tool] {} ok ({:.2}s)\n{}",
                self.context.tool_name,
                duration.as_secs_f64(),
                result
            );
        } else {
            app_println!(
                "[tool] {} ok ({:.2}s) {}",
                self.context.tool_name,
                duration.as_secs_f64(),
                result.lines().next().unwrap_or("").trim()
            );
        }
        crate::output::flush();
    }

    fn complete_error(&mut self, error: &str) {
        let duration = self.context.start_time.elapsed();
        app_eprintln!(
            "[tool] {} failed ({:.2}s) {}",
            self.context.tool_name,
            duration.as_secs_f64(),
            error.lines().next().unwrap_or("").trim()
        );
        crate::output::flush();
    }
}
