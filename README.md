# SHAFT

Secure Host Asynchronous File Transfer (i thought shaft was funny this acronym is forced :sob: )
FileZilla like TUI file transfer utility over SSH. Made with Claude. I needed an easy way to transfer files to servers through termianl and i got tired of typing scp commands.

## Development Status

BETA - in active development, many bugs, core functionality works (Copying files)

## Operating System Support

- Linux
- Windows (WIP)

## Usage

```
$ shaft user@host:port

$ shaft ssh-config-alias
```

## Installation

```
$ cargo install --git https://github.com/leadedspunk/shaft
```

## Known Issues

- Key Passphrase Authentication failing on windows
- Delete file not working
