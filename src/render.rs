//! macroquad rendering for the engine's retained display list.

use crate::abi::*;
use macroquad::prelude::*;

pub fn color(index: u8) -> Color {
    let [r, g, b] = palette_color(index);
    Color::from_rgba(r, g, b, 255)
}

fn set_world_camera() {
    let sw = screen_width().max(1.0);
    let sh = screen_height().max(1.0);
    let scale = (sw / CANVAS_W).min(sh / CANVAS_H);
    let camera = Camera2D {
        target: vec2(CANVAS_W / 2.0, CANVAS_H / 2.0),
        zoom: vec2(2.0 * scale / sw, 2.0 * scale / sh),
        ..Default::default()
    };
    set_camera(&camera);
}

/// Draw one retained frame.  The display list is replayed on every host
/// frame, which is what makes partial updates from Brainfuck cheap.
pub fn draw(commands: &[DrawCmd]) {
    clear_background(Color::from_rgba(4, 6, 10, 255));
    set_world_camera();

    // If the program did not begin with CLEAR, draw the canvas background.
    if !matches!(commands.first(), Some(DrawCmd::Clear { .. })) {
        draw_rectangle(0.0, 0.0, CANVAS_W, CANVAS_H, color(0));
    }

    for cmd in commands {
        match cmd {
            DrawCmd::Clear { color: c } => {
                draw_rectangle(0.0, 0.0, CANVAS_W, CANVAS_H, color(*c));
            }
            DrawCmd::Rect {
                x,
                y,
                w,
                h,
                color: c,
            } => {
                draw_rectangle(*x as f32, *y as f32, *w as f32, *h as f32, color(*c));
            }
            DrawCmd::RectLines {
                x,
                y,
                w,
                h,
                color: c,
            } => {
                draw_rectangle_lines(*x as f32, *y as f32, *w as f32, *h as f32, 3.0, color(*c));
            }
            DrawCmd::Line {
                x1,
                y1,
                x2,
                y2,
                color: c,
            } => {
                draw_line(
                    *x1 as f32,
                    *y1 as f32,
                    *x2 as f32,
                    *y2 as f32,
                    2.0,
                    color(*c),
                );
            }
            DrawCmd::Circle { x, y, r, color: c } => {
                draw_circle(*x as f32, *y as f32, *r as f32, color(*c));
            }
            DrawCmd::Text {
                x,
                y,
                color: c,
                size,
                text,
            } => {
                let size = (*size).clamp(8, 96) as f32;
                draw_text(text, *x as f32, *y as f32 + size * 0.82, size, color(*c));
            }
            DrawCmd::Number {
                x,
                y,
                value,
                color: c,
                size,
            } => {
                let size = (*size).clamp(8, 96) as f32;
                draw_text(
                    &value.to_string(),
                    *x as f32,
                    *y as f32 + size * 0.82,
                    size,
                    color(*c),
                );
            }
            DrawCmd::Cell {
                x,
                y,
                color: c,
                style,
            } => {
                draw_cell(*x, *y, *c, *style);
            }
            DrawCmd::Grid {
                x,
                y,
                cols,
                rows,
                cell,
                color: c,
            } => {
                let x = *x as f32;
                let y = *y as f32;
                let cell_size = *cell as f32;
                let w = *cols as f32 * cell_size;
                let h = *rows as f32 * cell_size;
                draw_rectangle(x, y, w, h, color(1));
                for i in 0..=*cols {
                    let gx = x + i as f32 * cell_size;
                    draw_rectangle(gx - 0.5, y, 1.0, h, color(*c));
                }
                for j in 0..=*rows {
                    let gy = y + j as f32 * cell_size;
                    draw_rectangle(x, gy - 0.5, w, 1.0, color(*c));
                }
            }
        }
    }

    set_default_camera();
}

fn draw_cell(x: u8, y: u8, color_index: u8, style: u8) {
    let px = CELL_ORIGIN_X + x as f32 * CELL_SIZE;
    let py = CELL_ORIGIN_Y + y as f32 * CELL_SIZE;
    let c = color(color_index);
    match style {
        1 => {
            // Food / dot.
            draw_circle(px + CELL_SIZE / 2.0, py + CELL_SIZE / 2.0, 8.0, c);
            draw_circle(
                px + CELL_SIZE / 2.0 - 2.0,
                py + CELL_SIZE / 2.0 - 2.5,
                2.2,
                Color::from_rgba(255, 255, 255, 130),
            );
        }
        2 => {
            // Head / highlighted block with eyes.
            draw_rectangle(px + 1.5, py + 1.5, CELL_SIZE - 3.0, CELL_SIZE - 3.0, c);
            let eye = color(0);
            draw_rectangle(px + 7.0, py + 7.0, 3.0, 3.0, eye);
            draw_rectangle(px + 14.0, py + 7.0, 3.0, 3.0, eye);
        }
        3 => {
            draw_rectangle_lines(px + 2.0, py + 2.0, CELL_SIZE - 4.0, CELL_SIZE - 4.0, 2.0, c);
        }
        _ => {
            draw_rectangle(px + 1.5, py + 1.5, CELL_SIZE - 3.0, CELL_SIZE - 3.0, c);
            draw_rectangle(
                px + 3.0,
                py + 3.0,
                CELL_SIZE - 6.0,
                2.0,
                Color::from_rgba(255, 255, 255, 28),
            );
        }
    }
}

pub fn draw_error(message: &str) {
    draw_rectangle(30.0, 500.0, 900.0, 110.0, Color::from_rgba(40, 8, 14, 245));
    draw_rectangle_lines(
        30.0,
        500.0,
        900.0,
        110.0,
        2.0,
        Color::from_rgba(239, 71, 111, 255),
    );
    draw_text(
        "ENGINE ERROR",
        50.0,
        532.0,
        24.0,
        Color::from_rgba(239, 71, 111, 255),
    );
    draw_text(
        &message.chars().take(100).collect::<String>(),
        50.0,
        566.0,
        18.0,
        Color::from_rgba(232, 240, 255, 255),
    );
    draw_text(
        "Press ESC / close the window to exit.",
        50.0,
        594.0,
        16.0,
        Color::from_rgba(143, 163, 191, 255),
    );
}
