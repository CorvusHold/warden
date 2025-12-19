//! Output formatting utilities for consistent CLI output
//!
//! Provides common output formatting for table, JSON, and YAML formats.

use std::io::{self, Write};

/// Output format options
#[allow(dead_code)] // Public API for output formatting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
}

#[allow(dead_code)] // Public API for output formatting
impl OutputFormat {
    /// Parse format string into OutputFormat
    #[allow(clippy::should_implement_trait)] // Simple conversion, not full FromStr
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "json" => OutputFormat::Json,
            "yaml" | "yml" => OutputFormat::Yaml,
            _ => OutputFormat::Table,
        }
    }

    /// Get the format name as a string
    pub fn as_str(&self) -> &'static str {
        match self {
            OutputFormat::Table => "table",
            OutputFormat::Json => "json",
            OutputFormat::Yaml => "yaml",
        }
    }
}

/// Table builder for consistent table output
#[allow(dead_code)] // Public API for table output
pub struct TableBuilder {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    show_header: bool,
}

#[allow(dead_code)] // Public API for table output
impl TableBuilder {
    /// Create a new table builder with the given headers
    pub fn new(headers: Vec<&str>) -> Self {
        Self {
            headers: headers.into_iter().map(|s| s.to_string()).collect(),
            rows: Vec::new(),
            show_header: true,
        }
    }

    /// Set whether to show the header row
    pub fn show_header(mut self, show: bool) -> Self {
        self.show_header = show;
        self
    }

    /// Add a row to the table
    pub fn add_row(&mut self, row: Vec<&str>) {
        self.rows
            .push(row.into_iter().map(|s| s.to_string()).collect());
    }

    /// Add a row with owned strings
    pub fn add_row_owned(&mut self, row: Vec<String>) {
        self.rows.push(row);
    }

    /// Build the table as a string
    pub fn build(&self) -> String {
        if self.headers.is_empty() && self.rows.is_empty() {
            return String::new();
        }

        // Calculate column widths
        let mut widths: Vec<usize> = self.headers.iter().map(|h| h.len()).collect();

        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        let mut output = String::new();

        // Header
        if self.show_header {
            let header_line: Vec<String> = self
                .headers
                .iter()
                .enumerate()
                .map(|(i, h)| format!("{:width$}", h, width = widths.get(i).copied().unwrap_or(0)))
                .collect();
            output.push_str(&header_line.join("  "));
            output.push('\n');

            // Separator
            let sep_line: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
            output.push_str(&sep_line.join("  "));
            output.push('\n');
        }

        // Rows
        for row in &self.rows {
            let row_line: Vec<String> = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    format!(
                        "{:width$}",
                        cell,
                        width = widths.get(i).copied().unwrap_or(0)
                    )
                })
                .collect();
            output.push_str(&row_line.join("  "));
            output.push('\n');
        }

        output
    }

    /// Print the table to stdout
    pub fn print(&self) {
        print!("{}", self.build());
        io::stdout().flush().ok();
    }
}

/// Format a key-value pair for display
#[allow(dead_code)] // Public API for output formatting
pub fn format_kv(key: &str, value: &str) -> String {
    format!("{}: {}", key, value)
}

/// Format a section header
#[allow(dead_code)] // Public API for output formatting
pub fn format_section(title: &str) -> String {
    format!("\n{}\n{}\n", title, "=".repeat(title.len()))
}

/// Format a subsection header
#[allow(dead_code)] // Public API for output formatting
pub fn format_subsection(title: &str) -> String {
    format!("\n{}\n{}\n", title, "-".repeat(title.len()))
}

/// Format bytes as human-readable size
#[allow(dead_code)] // Public API for output formatting
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format a duration in seconds as human-readable
#[allow(dead_code)] // Public API for output formatting
pub fn format_duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        let mins = seconds / 60;
        let secs = seconds % 60;
        if secs == 0 {
            format!("{}m", mins)
        } else {
            format!("{}m {}s", mins, secs)
        }
    } else if seconds < 86400 {
        let hours = seconds / 3600;
        let mins = (seconds % 3600) / 60;
        if mins == 0 {
            format!("{}h", hours)
        } else {
            format!("{}h {}m", hours, mins)
        }
    } else {
        let days = seconds / 86400;
        let hours = (seconds % 86400) / 3600;
        if hours == 0 {
            format!("{}d", days)
        } else {
            format!("{}d {}h", days, hours)
        }
    }
}

/// Status indicator with color (for terminal output)
#[allow(dead_code)] // Public API for status display
#[derive(Debug, Clone, Copy)]
pub enum Status {
    Ok,
    Warning,
    Error,
    Info,
}

#[allow(dead_code)] // Public API for status display
impl Status {
    /// Get the status indicator string
    pub fn indicator(&self) -> &'static str {
        match self {
            Status::Ok => "✓",
            Status::Warning => "⚠",
            Status::Error => "✗",
            Status::Info => "ℹ",
        }
    }

    /// Format a message with the status indicator
    pub fn format(&self, message: &str) -> String {
        format!("{} {}", self.indicator(), message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1048576), "1.00 MB");
        assert_eq!(format_bytes(1073741824), "1.00 GB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(3660), "1h 1m");
        assert_eq!(format_duration(86400), "1d");
        assert_eq!(format_duration(90000), "1d 1h");
    }

    #[test]
    fn test_table_builder() {
        let mut table = TableBuilder::new(vec!["Name", "Value"]);
        table.add_row(vec!["foo", "bar"]);
        table.add_row(vec!["longer_name", "x"]);

        let output = table.build();
        assert!(output.contains("Name"));
        assert!(output.contains("Value"));
        assert!(output.contains("foo"));
        assert!(output.contains("bar"));
    }

    #[test]
    fn test_output_format_from_str() {
        assert_eq!(OutputFormat::from_str("json"), OutputFormat::Json);
        assert_eq!(OutputFormat::from_str("JSON"), OutputFormat::Json);
        assert_eq!(OutputFormat::from_str("yaml"), OutputFormat::Yaml);
        assert_eq!(OutputFormat::from_str("yml"), OutputFormat::Yaml);
        assert_eq!(OutputFormat::from_str("table"), OutputFormat::Table);
        assert_eq!(OutputFormat::from_str("unknown"), OutputFormat::Table);
    }
}
