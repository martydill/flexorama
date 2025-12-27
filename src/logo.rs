/// ASCII art logo for aixplosion
pub static AIXPLOSION_LOGO: &str = r#"
     █████╗ ██╗██╗  ██╗██████╗ ██╗      ██████╗ ███████╗██╗ ██████╗ ███╗   ██╗
    ██╔══██╗██║╚██╗██╔╝██╔══██╗██║     ██╔═══██╗██╔════╝██║██╔═══██╗████╗  ██║
    ███████║██║ ╚███╔╝ ██████╔╝██║     ██║   ██║███████╗██║██║   ██║██╔██╗ ██║
    ██╔══██║██║ ██╔██╗ ██╔═══╝ ██║     ██║   ██║╚════██║██║██║   ██║██║╚██╗██║
    ██║  ██║██║██╔╝ ██╗██║     ███████╗╚██████╔╝███████║██║╚██████╔╝██║ ╚████║
    ╚═╝  ╚═╝╚═╝╚═╝  ╚═╝╚═╝     ╚══════╝ ╚═════╝ ╚══════╝╚═╝ ╚═════╝ ╚═╝  ╚═══╝
"#;

/// Minimal logo for very small terminals
pub static AIXPLOSION_LOGO_MINIMAL: &str = r#"
    ▄▀█ █ ▀▄▀ █▀█ █   █▀█ █▀ █ █▀█ █▄ █
    █▀█ █ █ █ █▀▀ █▄▄ █▄█ ▄█ █ █▄█ █ ▀█
"#;

/// Function to get the appropriate logo based on terminal width
pub fn get_logo_for_terminal() -> &'static str {
    // Try to get terminal width
    if let Ok((width, _)) = crossterm::terminal::size() {
        if width >= 80 {
            AIXPLOSION_LOGO
        } else {
            AIXPLOSION_LOGO_MINIMAL
        }
    } else {
        // Fallback to compact if we can't detect terminal size
        AIXPLOSION_LOGO_MINIMAL
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
        let subtitle = "?? Your Supercharged AI Coding Agent";
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
            colored_subtitle.push_str(&format!(
                "\x1b[38;2;{};{};{}m{}\x1b[0m",
                r, g, b, ch
            ));
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


