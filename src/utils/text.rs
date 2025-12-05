//! Text manipulation utilities

use unicode_width::UnicodeWidthStr;

/// Strip ANSI escape codes from a string
pub fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1B' {
            // ESC character - start of escape sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Skip until we hit a letter (the command character)
                while let Some(&next_ch) = chars.peek() {
                    chars.next();
                    if next_ch.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Get the visual width of a string (after stripping ANSI codes)
pub fn visual_width(s: &str) -> usize {
    let stripped = strip_ansi_codes(s);
    UnicodeWidthStr::width(stripped.as_str())
}

/// Wrap a line into multiple lines respecting visual width
/// For lines with colored icons, preserve the first line and only wrap the content
pub fn wrap_line(line: &str, max_width: usize) -> Vec<String> {
    let stripped = strip_ansi_codes(line);
    let width = UnicodeWidthStr::width(stripped.as_str());

    if width <= max_width {
        return vec![line.to_string()];
    }

    // Detect if this line has a colored icon (contains ANSI codes + emoji)
    let has_colored_icon = line.contains("\x1B[") && line.contains("🛈");

    // Detect leading whitespace/indentation
    let leading_spaces = stripped.chars().take_while(|c| c.is_whitespace()).count();
    let continuation_indent = "     "; // 5 spaces for continuation lines

    if has_colored_icon {
        // Special handling for colored icon lines
        // Split at the dash after the icon to separate rule name from message
        if let Some(dash_pos) = stripped.find(" - ") {
            let before_dash = &stripped[..dash_pos + 3]; // Include " - "
            let after_dash = &stripped[dash_pos + 3..];

            let before_width = UnicodeWidthStr::width(before_dash);

            if before_width <= max_width {
                // First line: preserve original up to dash (with colors)
                let original_before = if let Some(orig_dash) = line.find(" - ") {
                    &line[..orig_dash + 3]
                } else {
                    line
                };

                let mut result = vec![original_before.to_string()];

                // Wrap the rest
                let words: Vec<&str> = after_dash.split_whitespace().collect();
                let mut current_line = String::new();
                let mut current_width = 0;

                for word in words {
                    let word_width = UnicodeWidthStr::width(word);
                    let space_needed = if current_line.is_empty() { 0 } else { 1 };
                    let available_width = max_width.saturating_sub(continuation_indent.len());

                    if current_width + space_needed + word_width <= available_width {
                        if !current_line.is_empty() {
                            current_line.push(' ');
                            current_width += 1;
                        }
                        current_line.push_str(word);
                        current_width += word_width;
                    } else {
                        if !current_line.is_empty() {
                            result.push(format!("{}{}", continuation_indent, current_line));
                        }
                        current_line = word.to_string();
                        current_width = word_width;
                    }
                }

                if !current_line.is_empty() {
                    result.push(format!("{}{}", continuation_indent, current_line));
                }

                return result;
            }
        }
    }

    // Standard wrapping for non-icon lines
    let words: Vec<&str> = stripped.split_whitespace().collect();
    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;
    let mut is_first_line = true;

    for word in words {
        let word_width = UnicodeWidthStr::width(word);
        let space_needed = if current_line.is_empty() { 0 } else { 1 };
        let line_indent = if is_first_line {
            0
        } else {
            continuation_indent.len()
        };
        let available_width = max_width.saturating_sub(line_indent);

        if current_width + space_needed + word_width <= available_width {
            if !current_line.is_empty() {
                current_line.push(' ');
                current_width += 1;
            }
            current_line.push_str(word);
            current_width += word_width;
        } else {
            if !current_line.is_empty() {
                let final_line = if is_first_line {
                    format!(
                        "{}{}",
                        " ".repeat(leading_spaces),
                        current_line.trim_start()
                    )
                } else {
                    format!("{}{}", continuation_indent, current_line)
                };
                lines.push(final_line);
                is_first_line = false;
            }
            current_line = word.to_string();
            current_width = word_width;
        }
    }

    if !current_line.is_empty() {
        let final_line = if is_first_line {
            format!(
                "{}{}",
                " ".repeat(leading_spaces),
                current_line.trim_start()
            )
        } else {
            format!("{}{}", continuation_indent, current_line)
        };
        lines.push(final_line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}
