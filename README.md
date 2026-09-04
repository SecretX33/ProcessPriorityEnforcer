# Process Priority Enforcer

A small Windows utility that keeps selected applications in Efficiency mode.

On startup, it updates any matching processes that are already running, then monitors for newly started processes and updates them as they appear. Matching processes receive the configured priorities.

## Download

Process Priority Enforcer uses a lot of OS-native calls, so currently it's only available for Windows.

Get the latest version [here](https://github.com/SecretX33/SimpleBackup/releases/latest). Want an older version? Check all releases [here](https://github.com/SecretX33/SimpleBackup/releases).

## Usage

Leave the application running in the background to apply the configured priorities to matching processes whenever they start.

```sh
processpriorityenforcer.exe <path/to/config.json>
```

**Important:** run the app as administrator.

## Configuration

A JSON file containing groups of paths and priorities. The first group whose `paths` match a process is used:

```json
{
  "groups": [
    {
      "paths": [
        "C:/SomeFolder/**/*.exe",
        "**/SomeFile.exe"
      ],
      "priorities": {
        "cpu": "Idle",
        "io": "VeryLow",
        "power": "Eco"
      }
    }
  ]
}
```

The path patterns are case-sensitive and are matched against each process's full executable path. Both `/` and `\` path separators are accepted. The config file can be stored anywhere on your computer.

All `priorities` values are optional. When a value is unset, that priority is not changed. Enum values are case-insensitive.

- `cpu`: `Idle`, `BelowNormal`, `Normal`, `AboveNormal`, `High`
- `io`: `VeryLow`, `Low`, `Normal`
- `power`: `SystemManaged`, `Eco`, `High`

## Building from Source

- Install [Rust](https://rust-lang.org/tools/install/).
- Build the binary by executing this command, the compiled file will be in the `target/[debug|release]` folder.

```sh
# For development build
cargo build

# For release (optimized) build
cargo build --release
```

## License

This project is licensed under [MIT License](LICENSE).
