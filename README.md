# TimeMocker (Rust)

A Windows tool that injects fake time into running processes by hooking Win32 time APIs. Rust port of [time-mocker](https://github.com/tiennm99/time-mocker) (C# / EasyHook).

> **Status:** Active development. This Rust implementation is intended to become the canonical `time-mocker` once feature-complete; the C# project will be renamed to indicate its dotnet origin.

## Architecture

```
time-mocker-rs/
├── crates/
│   ├── time-mocker-core/   — shared types (MockTimeInfo) + named MMF helper + tick conversions
│   ├── time-mocker-hook/   — cdylib injected into target processes; hooks 5 time APIs via retour
│   └── time-mocker-ui/     — egui controller binary; injects via dll-syringe, writes delta per PID
```

## Hooked APIs

| API | DLL |
|-----|-----|
| `GetSystemTime` | kernel32 |
| `GetLocalTime` | kernel32 |
| `GetSystemTimeAsFileTime` | kernel32 |
| `GetSystemTimePreciseAsFileTime` | kernel32 |
| `NtQuerySystemTime` | ntdll |

## IPC Design

Named Memory-Mapped File per injected process:

```
Name: TimeMocker_<PID>
Size: 8 bytes
  [0..7]  DeltaTicks (i64 — 100-ns units, added to the real FILETIME)
```

The hook reads the delta on every time API call and returns `real_filetime + delta`. The controller writes the delta whenever the user picks a new fake time.

> **Tick epoch difference vs the C# version:** The C# version stores a delta against `DateTime.UtcNow.Ticks` (epoch 0001-01-01 UTC). The Rust version stores a delta in raw FILETIME units (epoch 1601-01-01 UTC). The two IPC contracts are not interoperable — the Rust UI and Rust hook DLL only talk to each other.

## Build

```powershell
# Nightly Rust (required by retour for inline x64 detours)
# A `rust-toolchain.toml` at the repo root pins the channel automatically.
cargo build --release

# Outputs:
#   target/release/time_mocker_ui.exe
#   target/release/time_mocker_hook.dll   (must be next to the UI exe)
```

## Requirements

- Windows 10/11 x64
- Rust nightly (pinned via `rust-toolchain.toml`)
- Must run as Administrator (UAC manifest embedded)

## License

MIT — see [LICENSE](LICENSE).

## Related

- [time-mocker](https://github.com/tiennm99/time-mocker) — original C# / EasyHook implementation
- [time-mocker-cpp](https://github.com/tiennm99/time-mocker-cpp) — C++ / MS Detours port
