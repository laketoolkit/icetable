//! Box frame utility for creating perfectly aligned text boxes

use crate::utils::visual_width;

/// Create a framed box with optional title and content lines
///
/// All lines will be exactly the same width from │ to │
///
/// # Arguments
/// * `title` - Optional title to display in the top border (e.g., "Arrow Diff")
/// * `lines` - Content lines to display in the box
/// * `width` - Fixed width of the box (default 72 if not specified)
///
/// # Returns
/// A formatted string with the framed content where all lines are perfectly aligned
///
/// # Example
/// ```
/// use icectl::utils::create_box_frame;
/// let frame = create_box_frame(
///     Some("Arrow Diff"),
///     vec!["file1.arrow → file2.arrow".to_string()],
///     Some(72)
/// );
/// ```
pub fn create_box_frame(title: Option<&str>, lines: Vec<String>, width: Option<usize>) -> String {
    let box_width = width.unwrap_or(72);
    let mut output = Vec::new();

    // Top border with optional title
    // Total width should be box_width + 2 (for the border chars │ on each side)
    if let Some(t) = title {
        // Format: ╭─ Title ───...───╮
        // ╭ = 1, ─ = 1, space = 1, title, space = 1, dashes, ╮ = 1
        // Total = 1 + 1 + 1 + title.len() + 1 + dashes + 1 = box_width + 2
        // So: 3 + title.len() + 1 + dashes + 1 = box_width + 2
        // dashes = box_width + 2 - 5 - title.len() = box_width - 3 - title.len()

        let title_len = visual_width(t);
        let dashes_needed = box_width.saturating_sub(3 + title_len);

        output.push(format!("╭─ {} {}╮", t, "─".repeat(dashes_needed)));
    } else {
        output.push(format!("╭{}╮", "─".repeat(box_width)));
    }

    // Content lines - all must be exactly box_width between the │ chars
    for line in lines {
        let line_visual_width = visual_width(&line);
        let padding_needed = box_width.saturating_sub(line_visual_width);

        output.push(format!("│{}{}│", line, " ".repeat(padding_needed)));
    }

    // Bottom border
    output.push(format!("╰{}╯", "─".repeat(box_width)));

    output.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::strip_ansi_codes;

    #[test]
    fn test_visual_width() {
        assert_eq!(visual_width("hello"), 5);
        assert_eq!(visual_width("hello world"), 11);
    }

    #[test]
    fn test_strip_ansi_codes() {
        let colored = "\x1B[31mred\x1B[0m";
        assert_eq!(strip_ansi_codes(colored), "red");
    }

    #[test]
    fn test_create_box_frame_no_title() {
        let frame = create_box_frame(None, vec!["hello".to_string()], Some(10));

        let lines: Vec<&str> = frame.lines().collect();
        assert_eq!(lines.len(), 3); // top, content, bottom

        // Check that all lines have the same total width
        // ╭──────────╮ = 12 chars (10 dashes + 2 corners)
        // │hello     │ = 12 chars
        // ╰──────────╯ = 12 chars
        assert_eq!(visual_width(lines[0]), 12);
        assert_eq!(visual_width(lines[1]), 12);
        assert_eq!(visual_width(lines[2]), 12);
    }

    #[test]
    fn test_create_box_frame_with_title() {
        let frame = create_box_frame(Some("Test"), vec!["content".to_string()], Some(20));

        let lines: Vec<&str> = frame.lines().collect();

        // All lines should have the same visual width
        let width0 = visual_width(lines[0]);
        let width1 = visual_width(lines[1]);
        let width2 = visual_width(lines[2]);

        assert_eq!(width0, width1);
        assert_eq!(width1, width2);
    }
}
