use crate::data::Readout;
use crate::theme::Theme;
use crate::widgets::readout::ReadoutList;
use atty::Stream;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Margin, Position, Rect};
use ratatui::text::Text;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use std::io;
use std::io::Stdout;
use unicode_width::UnicodeWidthStr;

pub fn create_backend() -> CrosstermBackend<Stdout> {
    CrosstermBackend::new(io::stdout())
}

pub fn find_widest_cell(buf: &Buffer, last_y: u16) -> u16 {
    let area = &buf.area;
    let mut widest: u16 = 0;
    let empty_cell = Cell::default();

    for y in 0..last_y {
        for x in (0..area.width).rev() {
            let current_cell = &buf[(x, y)];
            if current_cell.ne(&empty_cell) && x > widest {
                widest = x;
                break;
            }
        }
    }

    widest + 1
}

pub fn find_last_buffer_cell_index(buf: &Buffer) -> Option<(u16, u16)> {
    let empty_cell = Cell::default();

    if let Some((idx, _)) = buf
        .content
        .iter()
        .enumerate()
        .rfind(|p| !(*(p.1)).eq(&empty_cell))
    {
        return Some(buf.pos_of(idx));
    }

    None
}

pub fn draw_ascii(ascii: Text<'static>, tmp_buffer: &mut Buffer) -> Rect {
    let ascii_rect = Rect {
        x: 1,
        y: 1,
        width: ascii.width() as u16,
        height: ascii.height() as u16,
    };

    Paragraph::new(ascii).render(ascii_rect, tmp_buffer);
    ascii_rect
}

pub fn draw_readout_data(data: Vec<Readout>, theme: Theme, buf: &mut Buffer, area: Rect) {
    let mut list = ReadoutList::new(data, &theme);

    if theme.get_block().is_visible() {
        list = list
            .block_inner_margin(Margin {
                horizontal: theme.get_block().get_horizontal_margin(),
                vertical: theme.get_block().get_vertical_margin(),
            })
            .block(
                Block::default()
                    .border_type(theme.get_block().get_border_type())
                    .title(theme.get_block().get_title())
                    .borders(Borders::ALL),
            );
    }

    list.render(area, buf);
}

pub fn write_buffer_to_console(
    backend: &mut CrosstermBackend<Stdout>,
    tmp_buffer: &mut Buffer,
    skip_rect: Option<Rect>,
) -> Result<u16, io::Error> {
    let term_size = backend.size().unwrap_or_default();

    let (_, last_y) = find_last_buffer_cell_index(tmp_buffer)
        .expect("An error occurred while writing to the terminal buffer.");

    // When an image is being injected directly to stdout (skip_rect), its
    // footprint can be taller than whatever text ended up in the buffer
    // (e.g. a short readout list next to a tall image). If we only reserve
    // blank lines for the text, the image spills past them into whatever
    // gets printed next (e.g. the following shell prompt). Reserve enough
    // blank lines for the image's full height too.
    let last_y = last_y.max(
        skip_rect.map_or(0, |r| r.y.saturating_add(r.height).saturating_sub(1)),
    );

    let last_x = find_widest_cell(tmp_buffer, last_y);

    print!("{}", "\n".repeat(last_y as usize + 1));

    let mut cursor_y: u16 = 0;

    if atty::is(Stream::Stdout) {
        cursor_y = backend
            .get_cursor_position()
            .unwrap_or(Position { x: 0, y: 0 })
            .y;
    }

    // we need a checked subtraction here, because (cursor_y - last_y - 1) might underflow if the
    // cursor_y is smaller than (last_y - 1).
    let starting_pos = cursor_y.saturating_sub(last_y).saturating_sub(1);
    let mut skip_n = 0;

    let iter = tmp_buffer
        .content
        .iter()
        .enumerate()
        .filter(|(_previous, cell)| {
            let curr_width = cell.symbol().width();
            if curr_width == 0 {
                return false;
            }

            let old_skip = skip_n;
            skip_n = curr_width.saturating_sub(1);
            old_skip == 0
        })
        .map(|(idx, cell)| {
            let (x, y) = tmp_buffer.pos_of(idx);
            (x, y, cell)
        })
        .filter(|(x, y, _)| *x < last_x && *x < term_size.width && *y <= last_y)
        .filter(|(x, y, _)| {
            skip_rect.map_or(true, |r| {
                *x < r.x || *x >= r.x + r.width || *y < r.y || *y >= r.y + r.height
            })
        })
        .map(|(x, y, cell)| (x, y + starting_pos, cell));

    backend.draw(iter)?;
    Ok(starting_pos)
}
