# TimeMocker

A Windows tool that injects fake time into running processes by hooking Win32 time APIs. Written in Rust.

> **Status:** Active development. This is the canonical implementation. Language-specific ports are available in [C# (EasyHook)](https://github.com/tiennm99/time-mocker-csharp) and [C++ (MS Detours)](https://github.com/tiennm99/time-mocker-cpp).

## Architecture

```
time-mocker-rs/
├── crates/
│   ├── time-mocker-core/         — shared types (MockTimeInfo) + named MMF helper + tick conversions
│   ├── time-mocker-hook/         — cdylib injected into target processes; hooks 5 time APIs via retour
│   ├── time-mocker-test-target/  — console binary that prints all 5 hooked APIs in a loop (dedicated dev target)
│   └── time-mocker-ui/           — egui controller binary; injects via dll-syringe, writes delta per PID
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
Name: Global\TimeMocker_<PID>   (preferred — requires elevation)
      Local\TimeMocker_<PID>    (fallback — same-session, unelevated dev)
Size: 8 bytes
  [0..7]  DeltaTicks (i64 — 100-ns units, added to the real FILETIME)
```

The UI tries `Global\` first and falls back to `Local\` on `ERROR_ACCESS_DENIED`
(i.e. when the controller is not elevated and the token lacks
`SeCreateGlobalPrivilege`). The hook DLL probes both names. Release builds
embed a UAC manifest, so they always get `Global\`; debug builds run
unelevated and use `Local\` for same-session targets.

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
- Release build: must run as Administrator (UAC manifest embedded; needed for `Global\` MMF and cross-session injection)
- Debug build (`cargo run`): runs unelevated; falls back to `Local\` namespace — same-session targets only

## Dev workflow: test target

Inject into a dedicated harness instead of arbitrary running processes:

```powershell
# Terminal 1 — start the target, note the PID it prints
cargo run -p time-mocker-test-target

# Terminal 2 — start the UI; type that PID into "Inject by PID"
cargo build --workspace ; cargo run -p time-mocker-ui
```

The target prints all 5 hooked APIs (`GetSystemTime`, `GetLocalTime`,
`GetSystemTimeAsFileTime`, `GetSystemTimePreciseAsFileTime`,
`NtQuerySystemTime`) every second. When the hook is loaded and you set a
fake time in the UI, all five rows shift by the same delta — that's your
proof the hook is live.

## License

Apache-2.0 — see [LICENSE](LICENSE).

## Related

- [time-mocker-csharp](https://github.com/tiennm99/time-mocker-csharp) — C# / EasyHook implementation
- [time-mocker-cpp](https://github.com/tiennm99/time-mocker-cpp) — C++ / MS Detours implementation
