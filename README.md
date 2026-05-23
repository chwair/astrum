# Astrum
Simple starboard Discord bot written in Rust.

## Features
- Customizable "star" emoji
- Nice formatting for replies
- Support for all kinds of attachments, including voice

## How to use
`/config show` - shows current config (available for everyone)<br>
`/config channel` - set starboard channel<br>
`/config stars` - set minimum star count for a message to reach the starboard<br>
`/config emoji` - set emoji to be used for starring<br>

## How to run
0. Install [Rust](https://rust-lang.org/tools/install/) if you haven't already
1. Clone the repo
2. Copy .env.example as .env and add your Discord token
3. Run using `cargo run` or install systemwide as a TUI program using `cargo install --path .` (make sure the .env file is available where you'll be running Astrum and that said directory can be written to)

## License
MIT
