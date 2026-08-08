mod example;
mod sbuffer;

use clap::Parser;

#[derive(Debug, clap::Parser)]
#[command(version, about, verbatim_doc_comment, disable_help_flag = true)]
/// trmnl-console sends terminal snapshots to a "TRMNL Console" private plugin webhook.
///
/// It simulates a virtual terminal (PTY), runs a program inside it (or gets input data from stdin),
/// and then after a specified period of time (or when the app/stdin is closed; see details below)
/// it sends the snapshot of the terminal to the webhook for display on the device. It supports
/// other output modes too, such as an interactive live preview.
///
/// # Input Modes
/// - command mode: trmnl-console can be launched with a `[COMMAND]` to execute.
///   If given trmnl-console launches this command in the virtual terminal. Its stdout has control
///   over the terminal, and its stderr is forwarded to the stderr of trmnl-console itself.
///   In this mode trmnl-console will take the snapshot after `--wait-time` has elapsed, or the
///   command has exited with exit code 0. If the command exits with any other exit code,
///   trmnl-console will print an error message to stderr and exit with the same exit code.
/// - stdin mode: If no `[COMMAND]` is given and stdin is not a TTY (= you pipe input into
///   trmnl-console), then trmnl-console will instead send bytes from stdin to the virtual terminal.
///   In this mode trmnl-console will take the snapshot after `--wait-time` has elapsed, or stdin
///   is closed.
/// - demo mode: If no `[COMMAND]` is given and stdin is a TTY, trmnl-console will render an
///   example pattern into the virtual terminal.
///
/// # Output Modes
/// - with `--url`: The terminal snapshot is sent to the "TRMNL Console" webhook endpoint at the
///   given URL. The format of the payload structure is described at
///   https://docs.trmnl.com/go/private-plugins/webhooks. The contents of the "merge_variables" are
///   described in the section "JSON Output". Two requests are sent: The first updates
///   the metadata (everything except "data.content"), the second sets "data.content". The payload
///   may be rejected by the TRMNL servers if it is too big (see documentation). While the content
///   is compressed this may still happen with large colorful terminal contents. In this case
///   trmnl-console will output an error message to stderr and exit with exit code 90.
/// - with `--preview`: After taking the snapshot trmnl-console will launch a web server and try
///   to open a tab in your web browser to navigate to it. This page serves a preview of the
///   rendered terminal output as it would be displayed by the "TRMNL Console" plugin.
///   The server binds an OS-assigned port on 127.0.0.1 and prints the preview URL as a line to
///   stdout. Setting the environment variable `TRMNL_CONSOLE_NO_OPEN=1` prevents opening the
///   browser tab.
/// - with `--json`: The JSON payload that would be sent to the "TRMNL Console" plugin via webhook
///   is printed to stdout after taking the snapshot. See "JSON Output" section.
/// - otherwise: trmnl-console will output the HTML representation of the virtual terminal to
///   stdout. See "HTML Structure".
///
/// # HTML Structure
/// When none of `--url`, `--preview` or `--json` is given, trmnl-console will output the HTML
/// representation of the virtual terminal which would also be rendered by the "TRMNL Console"
/// plugin. This can be used to implement your own custom frontend for rendering the terminal
/// output.
///
/// The output is intended to be placed into a `<pre>` element. It contains the cells of the
/// terminal and `<span>` elements that apply formatting. The `<span>` elements may have HTML
/// classes that apply terminal attributes as described by the table below.
///
/// | Class    | Description                                                           |
/// |----------|-----------------------------------------------------------------------|
/// | tc--fg-X | Cells use xterm 256-color palette index X for their foreground color. |
/// | tc--bg-X | Cells use xterm 256-color palette index X for their background color. |
/// | tc--bold | Cells contain bold text.                                              |
/// | tc--dim  | Cells contain dimmed text.                                            |
/// | tc--ital | Cells contain italic text.                                            |
/// | tc--undl | Cells contain underlined text.                                        |
///
/// # JSON Output
/// The JSON payload sent via `--url` and printed via `--json` has the following structure:
///
/// ```text
/// - data:
///   - width: integer, see `--width`
///   - scale: integer, see `--scale`
///   - bar: object if any of the `--bar-*` options were used, otherwise null
///     - left: string or null, see `--bar-left`
///     - right: string or null, see `--bar-right`
///     - icon: string or null, see `--bar-icon`
///   - content: compressed "SBuffer" string representation of the terminal buffer,
///              see `src/sbuffer.rs` in the implementation of `trmnl-console` for the
///              specification.
/// ```
///
/// # Exit Codes
///
/// - 0: The command was successful.
/// - 90: TRMNL did not accept the webhook payload (or a network error occurred).
/// - 91: trmnl-console encountered an error.
/// - 101: trmnl-console encountered a critical, unhandled error (panic).
/// - any non-zero: In input mode "command mode" trmnl-console will exit with the non-zero exit
///   code of the command that was launched in the virtual terminal. This may also result in
///   any of the codes documented above.
pub struct Args {
    /// Show help information
    #[arg(long, action = clap::ArgAction::HelpLong)]
    help: Option<bool>,
    /// The number of columns of the virtual terminal.
    #[arg(short, long, value_name = "COLS")]
    pub width: u16,
    /// The number of rows of the virtual terminal.
    #[arg(short, long, value_name = "ROWS")]
    pub height: u16,
    /// The scale multiplier of the size of cells rendered on the device. Maximum value is 9.
    #[arg(long, default_value = "1", value_parser = clap::value_parser!(u8).range(1..=9))]
    pub scale: u8,
    /// The time in seconds to wait until creating the snapshot of the terminal and exiting.
    ///
    /// If a subprocess is spawned by trmnl-console it is killed when the timeout is reached.
    /// If the command instead exits before the timeout is reached, trmnl-console will take the
    /// snapshot at the moment of exit (if the exit code is 0).
    ///
    /// If trmnl-console is instead driven by drawing from stdin, the snapshot is taken when the
    /// timeout occurs or stdin is closed.
    #[arg(long)]
    pub wait_time: Option<f32>,
    /// TRMNL webhook URL to send the terminal snapshot to.
    /// Cannot be combined with --preview, --json.
    #[arg(short, long, value_name = "WEBHOOK_URL", group = "output")]
    pub url: Option<String>,
    /// Host a web server to preview the output and open it in a web browser.
    /// Cannot be combined with --url, --json.
    #[arg(short, long, group = "output")]
    pub preview: bool,
    /// Output the JSON sent to the TRMNL servers, including the encoded compressed virtual
    /// terminal snapshot.
    /// This is only useful for testing / development purposes.
    /// Cannot be combined with --url, --preview.
    #[arg(long, group = "output")]
    pub json: bool,
    /// Set the left title on the title bar.
    /// Providing any --bar-* option enables the title bar on the device.
    #[arg(long, value_name = "TITLE")]
    pub bar_left: Option<String>,
    /// Set the right (instance) title on the title bar.
    /// Providing any --bar-* option enables the title bar on the device.
    #[arg(long, value_name = "INSTANCE_TITLE")]
    pub bar_right: Option<String>,
    /// Set the image URL of the icon to show on the title bar.
    /// Providing any --bar-* option enables the title bar on the device.
    #[arg(long, value_name = "IMAGE_URL")]
    pub bar_icon: Option<String>,
    /// The command to execute. If not set, character data to be displayed is read from stdin
    /// instead. If the command contains flags that should not be parsed by trmnl-console,
    /// separate the options of trmnl-console and the program by a double dash "--". Example:
    /// `trmnl-console -w 10 -h 10 -- my-awesome-program -w 9`
    pub command: Option<Vec<String>>,
}

#[tokio::main]
pub async fn main() {
    let args = Args::parse();

    println!("{:?}", args);

    //example::main().await;
}
