# Astrum
Simple starboard Discord bot written in Rust.

## Features
- Everything in one message, built with Discord's Components v2: no embeds, no accent colours, laid out to read like a normal message
- Up to 10 customizable "star" emojis, managed from a button panel
- Author and reply avatars shown inline, cropped into circles
- Live star counts, edited in place as more people react
- Nice formatting for replies and forwards
- Support for all kinds of attachments: images, video, files, stickers and voice

## How to use
`/config show` - shows current config (available for everyone)<br>
`/config channel` - set starboard channel<br>
`/config stars` - set minimum star count for a message to reach the starboard<br>
`/config emoji` - add or remove the emojis used for starring<br>

## How to run
0. Install [Rust](https://rust-lang.org/tools/install/) if you haven't already
1. Clone the repo
2. Copy .env.example as .env and add your Discord token
3. Enable the **Message Content Intent** for your app under Bot → Privileged Gateway Intents in the [developer portal](https://discord.com/developers/applications), otherwise Discord closes the gateway on startup
4. Run using `cargo run` or install systemwide as a TUI program using `cargo install --path .` (make sure the .env file is available where you'll be running Astrum and that said directory can be written to)

Prebuilt Windows and Linux binaries are attached to each [release](https://github.com/chwair/astrum/releases).

Astrum keeps its config in `data.json` next to the executable, along with a record of the avatar emojis it uploads to your application (up to 1900, least recently used ones are dropped).

## License
MIT
