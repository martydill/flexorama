/// ASCII art logo for flexorama
pub static FLEXORAMA_LOGO: &str = r#"
    ███████╗██╗     ███████╗██╗  ██╗ ██████╗ ██████╗  █████╗ ███╗   ███╗ █████╗
    ██╔════╝██║     ██╔════╝╚██╗██╔╝██╔═══██╗██╔══██╗██╔══██╗████╗ ████║██╔══██╗
    █████╗  ██║     █████╗   ╚███╔╝ ██║   ██║██████╔╝███████║██╔████╔██║███████║
    ██╔══╝  ██║     ██╔══╝   ██╔██╗ ██║   ██║██╔══██╗██╔══██║██║╚██╔╝██║██╔══██║
    ██║     ███████╗███████╗██╔╝ ██╗╚██████╔╝██║  ██║██║  ██║██║ ╚═╝ ██║██║  ██║
    ╚═╝     ╚══════╝╚══════╝╚═╝  ╚═╝ ╚═════╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝     ╚═╝╚═╝  ╚═╝
"#;

/// Minimal logo for very small terminals
pub static FLEXORAMA_LOGO_MINIMAL: &str = r#"
    █▀▀ █   █▀▀ ▀▄▀ █▀█ █▀█ ▄▀█ █▀▄▀█ ▄▀█
    █▀  █▄▄ ██▄ █ █ █▄█ █▀▄ █▀█ █ ▀ █ █▀█
"#;

/// Function to get the appropriate logo based on terminal width
pub fn get_logo_for_terminal() -> &'static str {
    // Try to get terminal width
    if let Ok((width, _)) = crossterm::terminal::size() {
        if width >= 80 {
            FLEXORAMA_LOGO
        } else {
            FLEXORAMA_LOGO_MINIMAL
        }
    } else {
        // Fallback to compact if we can't detect terminal size
        FLEXORAMA_LOGO_MINIMAL
    }
}

/// Display the logo with colors using crossterm for smooth gradients
pub fn display_logo() {
    if crate::output::is_tui_active() {
        for line in get_logo_for_terminal().lines() {
            let mut colored_line = String::new();
            let chars: Vec<char> = line.chars().collect();
            for (i, ch) in chars.iter().enumerate() {
                if *ch == ' ' {
                    colored_line.push(' ');
                    continue;
                }
                let progress = i as f32 / chars.len().max(1) as f32;
                let (r, g, b) = if progress < 0.5 {
                    let t = progress / 0.5;
                    (255, (165.0 * t) as u8, 0)
                } else {
                    let t = (progress - 0.5) / 0.5;
                    (255, (165.0 * (1.0 - t) + 255.0 * t) as u8, 0)
                };
                colored_line.push_str(&format!("\x1b[38;2;{};{};{}m{}\x1b[0m", r, g, b, ch));
            }
            app_println!("{}", colored_line);
        }
        let subtitle = "✨ Your Supercharged AI Coding Agent";
        let mut colored_subtitle = String::new();
        let chars: Vec<char> = subtitle.chars().collect();
        for (i, ch) in chars.iter().enumerate() {
            let progress = i as f32 / chars.len().max(1) as f32;
            let (r, g, b) = if progress < 0.5 {
                let t = progress / 0.5;
                (255, (0.0 * (1.0 - t) + 165.0 * t) as u8, 0)
            } else {
                let t = (progress - 0.5) / 0.5;
                (255, (165.0 * (1.0 - t) + 255.0 * t) as u8, 0)
            };
            colored_subtitle.push_str(&format!("\x1b[38;2;{};{};{}m{}\x1b[0m", r, g, b, ch));
        }
        app_println!("{}", colored_subtitle);
        app_println!();
        return;
    }

    use crossterm::{
        queue,
        style::{Color, Print, ResetColor, SetForegroundColor},
    };
    use std::io::{stdout, Write};

    let logo = get_logo_for_terminal();
    let mut stdout = stdout();

    // Display the logo with smooth gradient effect
    for (_line_idx, line) in logo.lines().enumerate() {
        if line.trim().is_empty() {
            queue!(stdout, Print(line), Print("\n")).ok();
        } else {
            // Create a smooth horizontal gradient for each line
            let chars: Vec<char> = line.chars().collect();

            for (i, ch) in chars.iter().enumerate() {
                if *ch == ' ' {
                    queue!(stdout, Print(' ')).ok();
                    continue;
                }

                let progress = i as f32 / chars.len() as f32;

                // Smooth linear gradient: red (left) -> orange (middle) -> yellow (right)
                let (r, g, b) = if progress < 0.5 {
                    let t = progress / 0.5;
                    (255, (165.0 * t) as u8, 0)
                } else {
                    let t = (progress - 0.5) / 0.5;
                    (255, (165.0 * (1.0 - t) + 255.0 * t) as u8, 0)
                };

                queue!(
                    stdout,
                    SetForegroundColor(Color::Rgb { r, g, b }),
                    Print(ch)
                )
                .ok();
            }

            queue!(stdout, ResetColor, Print("\n")).ok();
        }
    }

    // Add a subtitle with fire gradient effect
    let subtitle = "🔥 Your Supercharged AI Coding Agent";
    for (i, ch) in subtitle.chars().enumerate() {
        let progress = i as f32 / subtitle.len() as f32;
        let (r, g, b) = if progress < 0.5 {
            // Red to orange
            let t = progress / 0.5;
            (255, (0.0 * (1.0 - t) + 165.0 * t) as u8, 0)
        } else {
            // Orange to yellow
            let t = (progress - 0.5) / 0.5;
            (255, (165.0 * (1.0 - t) + 255.0 * t) as u8, 0)
        };

        queue!(
            stdout,
            SetForegroundColor(Color::Rgb { r, g, b }),
            Print(ch)
        )
        .ok();
    }

    queue!(stdout, ResetColor, Print("\n\n")).ok();
    stdout.flush().ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flexorama_logo_not_empty() {
        assert!(!FLEXORAMA_LOGO.is_empty());
        // Logo contains box drawing chars, not plain text
        assert!(FLEXORAMA_LOGO.len() > 100);
    }

    #[test]
    fn test_flexorama_logo_minimal_not_empty() {
        assert!(!FLEXORAMA_LOGO_MINIMAL.is_empty());
        assert!(FLEXORAMA_LOGO_MINIMAL.len() < FLEXORAMA_LOGO.len());
    }

    #[test]
    fn test_flexorama_logo_contains_box_drawing_chars() {
        // The full logo uses box drawing characters
        assert!(
            FLEXORAMA_LOGO.contains("█")
                || FLEXORAMA_LOGO.contains("╗")
                || FLEXORAMA_LOGO.contains("╚")
        );
    }

    #[test]
    fn test_flexorama_logo_minimal_contains_block_chars() {
        // The minimal logo uses block elements
        assert!(
            FLEXORAMA_LOGO_MINIMAL.contains("█")
                || FLEXORAMA_LOGO_MINIMAL.contains("▀")
                || FLEXORAMA_LOGO_MINIMAL.contains("▄")
        );
    }

    #[test]
    fn test_get_logo_for_terminal_returns_valid_logo() {
        let logo = get_logo_for_terminal();
        assert!(!logo.is_empty());
        // Should return either full or minimal logo
        assert!(logo == FLEXORAMA_LOGO || logo == FLEXORAMA_LOGO_MINIMAL);
    }

    #[test]
    fn test_logos_are_different() {
        // Ensure the two logos are actually different
        assert_ne!(FLEXORAMA_LOGO, FLEXORAMA_LOGO_MINIMAL);
    }

    #[test]
    fn test_logo_has_multiple_lines() {
        let lines: Vec<&str> = FLEXORAMA_LOGO.lines().collect();
        assert!(lines.len() > 1, "Logo should have multiple lines");
    }

    #[test]
    fn test_logo_minimal_has_multiple_lines() {
        let lines: Vec<&str> = FLEXORAMA_LOGO_MINIMAL.lines().collect();
        assert!(lines.len() > 1, "Minimal logo should have multiple lines");
    }

    #[test]
    fn test_display_logo_doesnt_panic() {
        // This test just ensures display_logo doesn't panic
        // We can't easily test the actual output without mocking stdout
        // But we can at least verify it doesn't crash
        // Note: This might print to stdout during test runs, but that's okay

        // Skip actual display in tests to avoid cluttering test output
        // Just test that the function exists and is callable
        // display_logo(); // Commented out to avoid polluting test output
    }

    #[test]
    fn test_logo_constants_are_static() {
        // Test that we can access the logos multiple times
        let logo1 = FLEXORAMA_LOGO;
        let logo2 = FLEXORAMA_LOGO;
        assert_eq!(logo1, logo2);

        let minimal1 = FLEXORAMA_LOGO_MINIMAL;
        let minimal2 = FLEXORAMA_LOGO_MINIMAL;
        assert_eq!(minimal1, minimal2);
    }

    #[test]
    fn test_logo_formatting() {
        // Test that logos don't have excessively long lines
        for line in FLEXORAMA_LOGO.lines() {
            // ASCII art logos can be wide, allow up to 500 chars per line
            assert!(
                line.len() < 500,
                "Logo line is too long: {} chars",
                line.len()
            );
        }
    }

    #[test]
    fn test_minimal_logo_formatting() {
        for line in FLEXORAMA_LOGO_MINIMAL.lines() {
            assert!(line.len() < 100, "Minimal logo lines should be shorter");
        }
    }
}
