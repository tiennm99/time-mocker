# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

TimeMocker is a Windows application that injects fake time into running processes by hooking Win32 time APIs. It consists of a WinForms UI controller and an EasyHook-based DLL that gets injected into target processes.

## Build Commands

```bash
# Build the solution (x64 only)
dotnet build TimeMocker.sln -c Release -p:Platform=x64

# Build Debug configuration
dotnet build TimeMocker.sln -c Debug -p:Platform=x64

# Clean and rebuild
dotnet clean TimeMocker.sln && dotnet build TimeMocker.sln -c Release -p:Platform=x64
```

**Important**: This is an x64-only project. The solution only has x64 platform configurations. Building for x86 or AnyCPU will fail.

**Output locations**:
- UI exe: `TimeMocker.UI/bin/x64/Release/net48/TimeMocker.exe`
- Hook DLL: `TimeMocker.UI/bin/x64/Release/net48/TimeMocker.Hook.dll`

The hook DLL must be next to the UI exe (automatically copied via ProjectReference).

## Architecture

```
TimeMocker.sln
├── TimeMocker.UI/        — WinForms controller (requires Admin elevation)
│   ├── Forms/MainForm.cs           — Main UI with process selection, time picker, pattern manager
│   ├── Core/InjectionManager.cs    — EasyHook-based injector, manages injected processes
│   ├── Core/ProcessWatcher.cs      — Background scanner for auto-inject rules (poll-based)
│   └── Core/SharedMemoryManager.cs — Named MMF for IPC with hook DLL
│
└── TimeMocker.Hook/      — DLL injected into target processes
    └── InjectionEntryPoint.cs      — Hooks 5 Win32 time APIs via EasyHook
```

### IPC Design

The fake time is stored in a **named Memory-Mapped File** (one per injected process):

- Name format: `TimeMocker_<PID>`
- Size: 12 bytes (`Marshal.SizeOf<MockTimeInfo>()`)
- Layout:
  - `[0..7]` FakeUtcTicks (Int64 — DateTime.Ticks)
  - `[8..11]` Enabled (Int32 — 0=passthrough, 1=mock)

The hook reads this on every time API call (~50 ns read, no syscall). The UI writes via `SharedMemoryManager.Write()`.

### Hooked APIs

| API | DLL |
|-----|-----|
| `GetSystemTime` | kernel32 |
| `GetLocalTime` | kernel32 |
| `GetSystemTimeAsFileTime` | kernel32 |
| `GetSystemTimePreciseAsFileTime` | kernel32 |
| `NtQuerySystemTime` | ntdll |

All hooked functions share the same `GetFakeUtc()` logic which reads from shared memory and either returns the fake time or real `DateTime.UtcNow` depending on the Enabled flag.

### Auto-Inject Pattern Matching

The `ProcessWatcher` supports both glob and regex patterns for matching process paths or names:

- Glob: `C:\Games\MyGame\*` or `*chrome*` (converted to regex internally)
- Regex: `^.*\\MyApp\.exe$`

Patterns are matched against both the full process path (`MainModule.FileName`) and process name (`ProcessName`).

## Key Components

### InjectionManager

- Manages the dictionary of injected processes (`Dictionary<int, InjectedProcess>`)
- Uses `RemoteHooking.Inject()` to inject `TimeMocker.Hook.dll` into target processes
- Creates a `SharedMemoryManager` instance per process for IPC
- Handles eject by disabling the mock (setting Enabled=0) before disposing shared memory

### SharedMemoryManager

- Creates a named memory-mapped file using `MemoryMappedFile.CreateOrOpen()`
- `MockTimeInfo` struct is blittable (sequential layout) for direct memory write
- Must be disposed when process is ejected

### ProcessWatcher

- Polls running processes every ~1.5 seconds (configurable)
- Tracks seen process IDs to avoid duplicate injections
- Auto-injects when a process matches any enabled rule
- Uses current `FakeUtc` and `MockEnabled` settings at injection time

### InjectionEntryPoint (Hook DLL)

- Implements `IEntryPoint` from EasyHook
- `Run()` method opens the named MMF, installs hooks, and keeps alive with a sleep loop
- Uses `LocalHook.Create()` with thread ACL set to exclude thread 0 (all threads)
- Hook delegates read the shared memory on each call (hot path optimized)
- Logs errors to `%TEMP%\TimeMocker.Hook.log`

## Development Notes

- **Target Framework**: .NET Framework 4.8 (pre-installed on Windows 10+)
- **Unsafe Code**: Both projects use `AllowUnsafeBlocks=true` for memory-mapped file operations
- **Dependencies**: EasyHook 2.7.7030.0 (available via NuGet)
- **Language Version**: C# 8

### Limitations

- **64-bit only**: 32-bit processes require a separate 32-bit hook DLL build
- **No hot-unload support**: EasyHook doesn't fully support DLL ejection; disable mock instead
- **Protected processes**: Anti-cheat services (Epic, BattlEye, etc.) will block injection
- **QueryPerformanceCounter**: Not affected (hardware register, cannot be hooked via EasyHook)

### Time Epoch

FILETIME epoch is January 1, 1601 (UTC). Hooked functions returning FILETIME convert from DateTime.Ticks using:
```csharp
var epoch = new DateTime(1601, 1, 1, 0, 0, 0, DateTimeKind.Utc);
long fileTimeTicks = (fakeUtc - epoch).Ticks;
```

## Common Tasks

### Add a new time API hook
1. Add the hook delegate in `InjectionEntryPoint.cs` with `[UnmanagedFunctionPointer(CallingConvention.StdCall)]`
2. Add a `LocalHook` field for the hook handle
3. Create the hook in `InstallHooks()` using `LocalHook.GetProcAddress(dllName, functionName)`
4. Implement the hook function that calls `GetFakeUtc()` and returns the appropriate format

### Modify auto-inject polling interval
Pass a different interval to `ProcessWatcher.Start(int pollIntervalMs)` when enabling the watcher in `MainForm.cs`.

### Debug hook DLL
The hook writes errors to `%TEMP%\TimeMocker.Hook.log`. For more detailed debugging, you may need to attach a debugger to the target process after injection.
