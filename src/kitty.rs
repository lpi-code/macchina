use base64::{engine::general_purpose::STANDARD, Engine};
use image::ImageFormat;
use std::io::{self, Write};
use std::path::Path;

pub fn is_supported() -> bool {
    std::env::var("TERM").map_or(false, |v| v == "xterm-kitty")
        || std::env::var("KITTY_WINDOW_ID").is_ok()
        || std::env::var("TERM_PROGRAM").map_or(false, |v| v == "WezTerm")
}

/// Renders pre-captured chafa output at the given 1-indexed terminal position.
/// Handles both sixel (single DCS block) and symbol/ANSI (line-by-line) output.
pub fn render_chafa(output: &[u8], term_row: u16, term_col: u16) -> io::Result<()> {
    // Strip cursor hide/show sequences chafa emits that we handle ourselves.
    let cleaned = String::from_utf8_lossy(output)
        .replace("\x1b[?25l", "")
        .replace("\x1b[?25h", "");
    let cleaned = cleaned.as_bytes();

    let is_sixel = cleaned.windows(2).any(|w| w == b"\x1bP");

    let mut out = io::stdout().lock();
    write!(out, "\x1b[s\x1b[{};{}H", term_row, term_col)?;

    if is_sixel {
        // Sixel DCS block: send as-is at the cursor position.
        out.write_all(cleaned)?;
    } else {
        // Symbol/ANSI art: reposition cursor for each line so columns stay aligned.
        for (i, line) in cleaned.split(|&b| b == b'\n').enumerate() {
            if line.is_empty() {
                continue;
            }
            write!(out, "\x1b[{};{}H", term_row + i as u16, term_col)?;
            out.write_all(line)?;
        }
    }

    // Reset color, restore cursor, ensure cursor is visible.
    write!(out, "\x1b[0m\x1b[u\x1b[?25h")?;
    out.flush()?;
    Ok(())
}

/// Renders an image at the given 1-indexed terminal position using the Kitty graphics protocol.
pub fn render(path: &Path, cols: u16, rows: u16, term_row: u16, term_col: u16) -> io::Result<()> {
    let img = image::open(path)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;

    // Scale the image to approximately fit the target cell area.
    // Typical terminal cell: ~10px wide, ~20px tall.
    let px_w = (cols as u32) * 10;
    let px_h = (rows as u32) * 20;
    let img = img.thumbnail(px_w, px_h);

    let mut png_data: Vec<u8> = Vec::new();
    img.write_to(&mut io::Cursor::new(&mut png_data), ImageFormat::Png)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    let encoded = STANDARD.encode(&png_data);
    let chunks: Vec<&[u8]> = encoded.as_bytes().chunks(4096).collect();
    let total = chunks.len();

    let mut out = io::stdout().lock();

    // Save cursor position, then move to the target terminal cell (1-indexed).
    write!(out, "\x1b[s\x1b[{};{}H", term_row, term_col)?;

    for (i, chunk) in chunks.iter().enumerate() {
        let chunk_str = std::str::from_utf8(chunk).unwrap();
        // m=1 means more chunks follow; m=0 means this is the last (or only) chunk.
        let more = u8::from(i < total - 1);
        if i == 0 {
            write!(
                out,
                "\x1b_Ga=T,f=100,q=2,c={},r={},m={};{}\x1b\\",
                cols, rows, more, chunk_str
            )?;
        } else {
            write!(out, "\x1b_Gm={};{}\x1b\\", more, chunk_str)?;
        }
    }

    // Restore cursor to where ratatui left it so the trailing newlines land correctly.
    write!(out, "\x1b[u")?;
    out.flush()?;

    Ok(())
}
