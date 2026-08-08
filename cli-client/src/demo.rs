const TRMNL_ASCII_ART: &str = "    .-:.   :=+.     
    -++++=..++=     
    ...:--  -++: :. 
.:-+++       .  -++:
:++-.         .+++: 
 . .::         .-.  
   =++      .:----: 
  .++= :+=: .+++==: 
   ::. .-++=:       
         .==.       ";
const TRMNL_ASCII_ART_DIM: (usize, usize) = (20, 10);
const BORDER_TL: &str = "╭";
const BORDER_TR: &str = "╮";
const BORDER_BR: &str = "╯";
const BORDER_BL: &str = "╰";
const BORDER_H: &str = "─";
const BORDER_V: &str = "│";

pub fn render_terminal_output(width: usize, height: usize) -> String {
    let top_row = format!("{}{}{}", BORDER_TL, BORDER_H.repeat(width - 2), BORDER_TR);
    let bottom_row = format!("{}{}{}", BORDER_BL, BORDER_H.repeat(width - 2), BORDER_BR);
    let start_row_trmnl_logo = if (width - 20, height) > TRMNL_ASCII_ART_DIM {
        Some(height / 2 - TRMNL_ASCII_ART_DIM.1 / 2)
    } else {
        None
    };

    let mut logo_buffer = TRMNL_ASCII_ART.lines().rev().collect::<Vec<&str>>();

    let mut lines = Vec::new();
    for line_i in 0..height - 2 {
        let line_i = line_i + 1;

        let line_content = if Some(line_i) >= start_row_trmnl_logo
            && let Some(logo_line) = logo_buffer.pop()
        {
            let rest_space = width
                .saturating_sub(2)
                .saturating_sub(5)
                .saturating_sub(TRMNL_ASCII_ART_DIM.0);
            format!(
                "     {}{}",
                logo_line,
                render_text(width, height, line_i, rest_space)
            )
        } else {
            render_text(width, height, line_i, width.saturating_sub(2))
        };

        lines.push(format!("{}{}{}", BORDER_V, line_content, BORDER_V));
    }
    format!("{}\n{}\n{}", top_row, lines.join("\n"), bottom_row)
}

fn render_text(width: usize, height: usize, line_i: usize, rest_spaces: usize) -> String {
    let header_row = (height / 2).saturating_sub(1);
    let description_row = height / 2;
    let description_row2 = height / 2 + 1;
    if line_i == header_row {
        let text = "  TRMNL Console Plugin";
        format!(
            "\x1B[1m{}\x1B[22m{}",
            text,
            " ".repeat(rest_spaces.saturating_sub(text.len()))
        )
    } else if line_i == description_row {
        let text = format!("  Output size: {width} x {height}");
        format!(
            "{}{}",
            text,
            " ".repeat(rest_spaces.saturating_sub(text.len()))
        )
    } else if line_i == description_row2 {
        let text = "  This is the demo output. Try adding ";
        let text2 = "-- your-command";
        let text3 = " to ";
        let text4 = "trmnl-console";
        let text5 = ".";
        let texts_len = text.len() + text2.len() + text3.len() + text4.len() + text5.len();
        format!(
            "{}\x1B[3m{}\x1B[23m{}\x1B[3m{}\x1B[23m{}{}",
            text,
            text2,
            text3,
            text4,
            text5,
            " ".repeat(rest_spaces.saturating_sub(texts_len))
        )
    } else {
        " ".repeat(rest_spaces)
    }
}
