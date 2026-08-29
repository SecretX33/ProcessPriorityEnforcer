# Process Priority Enforcer

A small Windows utility that keeps selected applications in Efficiency mode.

On startup, it updates any matching processes that are already running, then monitors for newly started processes and updates them as they appear. Matching processes receive idle CPU priority, very-low I/O priority, and Eco power QoS.

## Download

Process Priority Enforcer uses a lot of OS-native calls, so currently it's only available for Windows.

Get the latest version [here](https://github.com/SecretX33/SimpleBackup/releases/latest). Want an older version? Check all releases [here](https://github.com/SecretX33/SimpleBackup/releases).

## Usage

Leave the application running in the background to apply "Efficiency mode" to matching processes whenever they start.

```sh
processpriorityenforcer.exe <path/to/config.json>
```

**Important:** run the app as administrator.

## Configuration

A JSON file containing the absolute executable paths to match. Each entry supports glob patterns:

```json
{
  "paths": [
    "C:/SomeFolder/**/*.exe",
    "**/SomeFile.exe"
  ]
}
```

The patterns are **case-sensitive** and are matched against each process's full executable path (accepts both `/` and `\` path separators).  The config file can be stored anywhere on your computer.

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