#![warn(clippy::all)]

mod ascii;
mod bars;
mod buffer;
mod cli;
mod config;
mod data;
mod doctor;
mod error;
mod extra;
mod format;
mod kitty;
pub mod theme;
pub mod widgets;

use cli::{Opt, PKG_NAME};
use error::Result;
use ratatui::{backend::Backend, buffer::Buffer, layout::Rect};

#[macro_use]
extern crate lazy_static;

fn main() -> Result<()> {
    let opt = Opt::get_options();

    if opt.version {
        get_version();
        return Ok(());
    }

    if opt.ascii_artists {
        ascii::list_ascii_artists();
        return Ok(());
    }

    if opt.list_themes {
        theme::list_themes(&opt);
        return Ok(());
    }

    let theme = theme::create_theme(&opt);
    let should_display = data::should_display(&opt);
    let readout_data = data::get_all_readouts(&opt, &theme, &should_display);

    if opt.doctor {
        doctor::print_doctor(&readout_data);
        return Ok(());
    }

    const MAX_ASCII_HEIGHT: usize = 50;
    const MINIMUM_READOUTS_TO_PREFER_SMALL_ASCII: usize = 8;
    let mut backend = buffer::create_backend();
    let mut tmp_buffer = Buffer::empty(Rect::new(0, 0, 500, 50));
    let mut ascii_area = Rect::new(0, 1, 0, tmp_buffer.area.height - 1);
    let prefers_small_ascii =
        readout_data.len() < MINIMUM_READOUTS_TO_PREFER_SMALL_ASCII || theme.prefers_small_ascii();

    let image_cols = opt.image_size.unwrap_or(20);
    let image_rows = (readout_data.len() as u16 + 2).max(10);

    // Resolve the image path and decide the rendering backend.
    // Chafa output is captured here so we can inject it directly to stdout after ratatui renders,
    // bypassing the ratatui buffer (which loses 24-bit background colors).
    enum ImageMode {
        Kitty(std::path::PathBuf),
        Chafa(Vec<u8>),
        None,
    }

    let image_mode = match &opt.image {
        Some(p) => {
            let expanded = shellexpand::tilde(&p.to_string_lossy()).to_string();
            let path = std::path::PathBuf::from(expanded);
            if opt.force_kitty || kitty::is_supported() {
                ImageMode::Kitty(path)
            } else if which_chafa() {
                let size_arg = format!("{}x{}", image_cols, image_rows);
                // Windows Terminal supports sixel graphics (added in 1.22).
                // Other terminals without kitty fall back to Unicode block symbols.
                let fmt = if std::env::var("WT_SESSION").is_ok() { "sixels" } else { "symbols" };
                match std::process::Command::new("chafa")
                    .args([
                        "--size", &size_arg,
                        "--format", fmt,
                        "--passthrough", "none",
                        path.to_str().unwrap_or(""),
                    ])
                    .output()
                {
                    Ok(out) if !out.stdout.is_empty() => ImageMode::Chafa(out.stdout),
                    _ => ImageMode::None,
                }
            } else {
                ImageMode::None
            }
        }
        None => ImageMode::None,
    };

    match &image_mode {
        ImageMode::Kitty(_) | ImageMode::Chafa(_) => {
            // Reserve blank space; the image is injected directly to stdout after ratatui renders.
            ascii_area = Rect::new(1, 1, image_cols, image_rows);
        }
        ImageMode::None => {}
    }

    if matches!(image_mode, ImageMode::None) && theme.is_ascii_visible() {
        if let Some(path) = theme.get_custom_ascii().get_path() {
            let expanded = shellexpand::tilde(&path.to_string_lossy()).to_string();
            let file_path = std::path::PathBuf::from(expanded);
            let ascii_art = if let Some(color) = theme.get_custom_ascii().get_color() {
                ascii::get_ascii_from_file_override_color(&file_path, color)?
            } else {
                ascii::get_ascii_from_file(&file_path)?
            };

            if ascii_art.width() != 0 && ascii_art.height() < MAX_ASCII_HEIGHT {
                ascii_area = buffer::draw_ascii(ascii_art, &mut tmp_buffer);
            }
        } else if prefers_small_ascii {
            // prefer smaller ascii in this case
            if let Some(ascii) = ascii::select_ascii(ascii::AsciiSize::Small) {
                ascii_area = buffer::draw_ascii(ascii, &mut tmp_buffer);
            }
        } else {
            // prefer bigger ascii otherwise
            if let Some(ascii) = ascii::select_ascii(ascii::AsciiSize::Big) {
                ascii_area = buffer::draw_ascii(ascii, &mut tmp_buffer);
            }
        }
    }

    let tmp_buffer_area = tmp_buffer.area;

    buffer::draw_readout_data(
        readout_data,
        theme,
        &mut tmp_buffer,
        Rect::new(
            ascii_area.x + ascii_area.width + 2,
            ascii_area.y,
            tmp_buffer_area.width - ascii_area.width - 4,
            ascii_area.height,
        ),
    );

    let skip = if matches!(image_mode, ImageMode::Kitty(_) | ImageMode::Chafa(_)) {
        Some(ascii_area)
    } else {
        None
    };
    let starting_pos = buffer::write_buffer_to_console(&mut backend, &mut tmp_buffer, skip)?;

    backend.flush()?;

    // Both kitty and chafa inject directly to stdout so the full ANSI output reaches the terminal.
    // ascii_area starts at buffer (x=1, y=1); terminal coords are 1-indexed → (starting_pos+2, 2).
    let term_row = starting_pos + 2;
    match &image_mode {
        ImageMode::Kitty(ref img_path) => {
            if let Err(e) = kitty::render(img_path, image_cols, ascii_area.height, term_row, 2) {
                eprintln!("macchina: kitty image error: {e}");
            }
        }
        ImageMode::Chafa(ref chafa_bytes) => {
            if let Err(e) = kitty::render_chafa(chafa_bytes, term_row, 2) {
                eprintln!("macchina: chafa render error: {e}");
            }
        }
        ImageMode::None => {}
    }

    print!("\n\n");

    Ok(())
}

fn which_chafa() -> bool {
    std::process::Command::new("which")
        .arg("chafa")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn get_version() {
    if let Some(git_sha) = option_env!("VERGEN_GIT_SHA_SHORT") {
        println!(
            "{}     {} ({})",
            PKG_NAME,
            env!("CARGO_PKG_VERSION"),
            git_sha
        );
    } else {
        println!("{}     {}", PKG_NAME, env!("CARGO_PKG_VERSION"));
    }

    println!("libmacchina  {}", libmacchina::version());
}
