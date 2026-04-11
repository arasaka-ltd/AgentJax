use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use tokio::fs;

use crate::builtin::tools::ToolOutput;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewlineStyle {
    Lf,
    Crlf,
    Mixed,
}

impl NewlineStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "lf",
            Self::Crlf => "crlf",
            Self::Mixed => "mixed",
        }
    }

    pub fn encode(self, text: &str) -> String {
        match self {
            Self::Lf | Self::Mixed => text.to_string(),
            Self::Crlf => text.replace('\n', "\r\n"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextDocument {
    pub path: String,
    pub text: String,
    pub newline: NewlineStyle,
    line_starts: Vec<usize>,
}

impl TextDocument {
    pub async fn load(path: &str) -> Result<Self> {
        let bytes = fs::read(path).await?;
        Self::from_bytes(path, &bytes)
    }

    pub fn from_bytes(path: &str, bytes: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| anyhow!("file is not valid UTF-8 text: {path}"))?
            .to_string();
        let newline = detect_newline_style(&text);
        let normalized = normalize_newlines(&text);
        Ok(Self::new(path.to_string(), normalized, newline))
    }

    pub fn new(path: String, text: String, newline: NewlineStyle) -> Self {
        let mut line_starts = vec![0];
        for (idx, ch) in text.char_indices() {
            if ch == '\n' {
                line_starts.push(idx + 1);
            }
        }
        Self {
            path,
            text,
            newline,
            line_starts,
        }
    }

    pub fn total_lines(&self) -> usize {
        self.line_starts.len().max(1)
    }

    pub fn slice_lines(&self, start_line: usize, end_line: usize) -> Result<String> {
        if start_line == 0 || end_line == 0 {
            bail!("line numbers must be 1-based");
        }
        if start_line > end_line {
            bail!("start_line must be <= end_line");
        }
        if end_line > self.total_lines() {
            bail!("requested line range exceeds file length");
        }
        let mut rendered = Vec::new();
        for line_number in start_line..=end_line {
            rendered.push(format!(
                "{}| {}",
                line_number,
                self.line_content(line_number)?
            ));
        }
        Ok(rendered.join("\n"))
    }

    pub fn line_content(&self, line_number: usize) -> Result<&str> {
        if line_number == 0 || line_number > self.total_lines() {
            bail!("line number out of range");
        }
        let start = self.line_starts[line_number - 1];
        let end = if line_number == self.total_lines() {
            self.text.len()
        } else {
            self.line_starts[line_number] - 1
        };
        Ok(&self.text[start..end])
    }

    pub fn offset_for_position(&self, line: usize, column: usize) -> Result<usize> {
        if line == 0 || column == 0 {
            bail!("line and column must be 1-based");
        }
        if line > self.total_lines() {
            bail!("line out of range");
        }
        let start = self.line_starts[line - 1];
        let content = self.line_content(line)?;
        let char_len = content.chars().count();
        if column > char_len + 1 {
            bail!("column out of range");
        }
        if column == char_len + 1 {
            return Ok(start + content.len());
        }
        let char_offset = content
            .char_indices()
            .nth(column - 1)
            .map(|(idx, _)| idx)
            .ok_or_else(|| anyhow!("column out of range"))?;
        Ok(start + char_offset)
    }

    pub fn replace_range(
        &self,
        start_line: usize,
        start_column: usize,
        end_line: usize,
        end_column: usize,
        new_text: &str,
    ) -> Result<(String, Value)> {
        let start = self.offset_for_position(start_line, start_column)?;
        let end = self.offset_for_position(end_line, end_column)?;
        if start > end {
            bail!("edit start must be before or equal to end");
        }
        let mut updated = String::with_capacity(self.text.len() + new_text.len());
        updated.push_str(&self.text[..start]);
        updated.push_str(new_text);
        updated.push_str(&self.text[end..]);

        let inserted = TextDocument::new(self.path.clone(), new_text.to_string(), NewlineStyle::Lf);
        let new_range =
            compute_inserted_range(start_line, start_column, inserted.total_lines(), new_text);
        Ok((updated, new_range))
    }

    pub fn encoded_text(&self) -> String {
        self.newline.encode(&self.text)
    }
}

fn compute_inserted_range(
    start_line: usize,
    start_column: usize,
    inserted_lines: usize,
    new_text: &str,
) -> Value {
    if new_text.is_empty() {
        return json!({
            "start_line": start_line,
            "start_column": start_column,
            "end_line": start_line,
            "end_column": start_column,
        });
    }

    let segments: Vec<&str> = new_text.split('\n').collect();
    if inserted_lines <= 1 {
        let width = segments
            .first()
            .map(|line| line.chars().count())
            .unwrap_or(0);
        return json!({
            "start_line": start_line,
            "start_column": start_column,
            "end_line": start_line,
            "end_column": start_column + width,
        });
    }

    let last_width = segments
        .last()
        .map(|line| line.chars().count())
        .unwrap_or(0);
    json!({
        "start_line": start_line,
        "start_column": start_column,
        "end_line": start_line + inserted_lines - 1,
        "end_column": last_width + 1,
    })
}

pub fn parse_path(args: &Value, tool_name: &str) -> Result<String> {
    args.get("path")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow!("{tool_name} requires args.path"))
}

pub fn parse_required_usize(args: &Value, key: &str, tool_name: &str) -> Result<usize> {
    let value = args
        .get(key)
        .and_then(|value| value.as_u64())
        .ok_or_else(|| anyhow!("{tool_name} requires args.{key}"))?;
    usize::try_from(value).map_err(|_| anyhow!("{tool_name} args.{key} is too large"))
}

pub fn parse_optional_usize(args: &Value, key: &str) -> Result<Option<usize>> {
    let Some(value) = args.get(key).and_then(|value| value.as_u64()) else {
        return Ok(None);
    };
    Ok(Some(
        usize::try_from(value).map_err(|_| anyhow!("args.{key} is too large"))?,
    ))
}

pub fn parse_optional_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|value| value.as_bool())
}

pub fn parse_optional_string<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|value| value.as_str())
}

pub fn normalize_newlines(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if matches!(chars.peek(), Some('\n')) {
                chars.next();
            }
            normalized.push('\n');
        } else {
            normalized.push(ch);
        }
    }
    normalized
}

pub fn detect_newline_style(text: &str) -> NewlineStyle {
    let bytes = text.as_bytes();
    let mut saw_lf = false;
    let mut saw_crlf = false;
    let mut idx = 0;

    while idx < bytes.len() {
        match bytes[idx] {
            b'\r' if idx + 1 < bytes.len() && bytes[idx + 1] == b'\n' => {
                saw_crlf = true;
                idx += 2;
            }
            b'\n' => {
                saw_lf = true;
                idx += 1;
            }
            _ => idx += 1,
        }
    }

    match (saw_lf, saw_crlf) {
        (true, true) => NewlineStyle::Mixed,
        (false, true) => NewlineStyle::Crlf,
        _ => NewlineStyle::Lf,
    }
}

pub fn encode_with_style(text: &str, newline: NewlineStyle) -> String {
    newline.encode(text)
}

pub fn choose_write_newline(
    path: &str,
    requested: Option<&str>,
    exists: bool,
    text: &str,
) -> Result<NewlineStyle> {
    match requested.unwrap_or("preserve_if_exists") {
        "lf" => Ok(NewlineStyle::Lf),
        "crlf" => Ok(NewlineStyle::Crlf),
        "preserve_if_exists" => {
            if exists {
                Ok(detect_newline_style(text))
            } else {
                Ok(NewlineStyle::Lf)
            }
        }
        other => bail!("unsupported newline mode for {path}: {other}"),
    }
}

pub fn ensure_parent_dir(path: &str) -> Result<Option<PathBuf>> {
    Ok(Path::new(path).parent().map(Path::to_path_buf))
}

pub fn render_clipped_lines(
    content: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
    max_lines: Option<usize>,
) -> (String, usize, usize, usize, bool) {
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len().max(1);
    let start = start_line.unwrap_or(1).max(1).min(total_lines);
    let mut end = end_line.unwrap_or(total_lines).max(start).min(total_lines);
    if end_line.is_none() {
        if let Some(max_lines) = max_lines {
            end = (start + max_lines.saturating_sub(1)).min(total_lines);
        }
    }
    let rendered = lines
        .iter()
        .enumerate()
        .skip(start - 1)
        .take(end - start + 1)
        .map(|(idx, line)| format!("{}| {}", idx + 1, line))
        .collect::<Vec<_>>()
        .join("\n");
    (rendered, start, end, total_lines, end < total_lines)
}

pub fn json_tool_output(value: Value) -> Result<ToolOutput> {
    Ok(ToolOutput {
        content: serde_json::to_string_pretty(&value)?,
        metadata: value,
    })
}

pub fn image_metadata(path: &str, bytes: &[u8]) -> Result<Value> {
    let format = image::guess_format(bytes)?;
    let mime = match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::Gif => "image/gif",
        image::ImageFormat::Bmp => "image/bmp",
        image::ImageFormat::Ico => "image/x-icon",
        image::ImageFormat::WebP => "image/webp",
        _ => bail!("unsupported image format for {path}"),
    };
    let (width, height) = image::image_dimensions(path)?;
    Ok(json!({
        "path": path,
        "kind": "image",
        "mime_type": mime,
        "width": width,
        "height": height,
        "size_bytes": bytes.len(),
        "image_ref": format!("image:workspace/{path}"),
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        detect_newline_style, image_metadata, normalize_newlines, NewlineStyle, TextDocument,
    };

    #[test]
    fn detects_newline_styles() {
        assert_eq!(detect_newline_style("a\nb\n"), NewlineStyle::Lf);
        assert_eq!(detect_newline_style("a\r\nb\r\n"), NewlineStyle::Crlf);
        assert_eq!(detect_newline_style("a\nb\r\n"), NewlineStyle::Mixed);
    }

    #[test]
    fn normalizes_crlf_to_lf() {
        assert_eq!(normalize_newlines("a\r\nb\r\n"), "a\nb\n");
    }

    #[test]
    fn reads_png_metadata() {
        let png = vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92,
            0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let path = std::env::temp_dir().join("agentjax-test-image.png");
        std::fs::write(&path, &png).unwrap();
        let metadata = image_metadata(path.to_str().unwrap(), &png).unwrap();
        assert_eq!(metadata["kind"], "image");
        assert_eq!(metadata["mime_type"], "image/png");
        assert_eq!(metadata["width"], 1);
        assert_eq!(metadata["height"], 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn maps_positions_and_replaces_ranges() {
        let doc = TextDocument::new("test.txt".into(), "alpha\nbeta\n".into(), NewlineStyle::Lf);
        let (updated, range) = doc.replace_range(2, 2, 2, 4, "XYZ").unwrap();
        assert_eq!(updated, "alpha\nbXYZa\n");
        assert_eq!(
            range,
            json!({
                "start_line": 2,
                "start_column": 2,
                "end_line": 2,
                "end_column": 5,
            })
        );
    }
}
