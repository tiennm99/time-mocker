# TimeMocker

A Windows application that injects fake time into running processes by hooking Win32 time APIs.

## Architecture

```
TimeMocker.sln
├── TimeMocker.UI        — WinForms controller app (run as Admin)
│   ├── Forms/MainForm   — UI: process list with inject toggle, time picker, pattern manager
│   ├── Core/InjectionManager  — EasyHook-based injector per process
│   ├── Core/ProcessWatcher    — background scanner for auto-inject patterns
│   └── Core/SharedMemoryManager — named MMF shared with the hook DLL
│
└── TimeMocker.Hook      — DLL injected into target processes
    └── InjectionEntryPoint — hooks 5 Win32 time functions via EasyHook
```

## Hooked APIs

| API | DLL |
|-----|-----|
| `GetSystemTime` | kernel32 |
| `GetLocalTime`  | kernel32 |
| `GetSystemTimeAsFileTime` | kernel32 |
| `GetSystemTimePreciseAsFileTime` | kernel32 |
| `NtQuerySystemTime` | ntdll |

## Requirements

- **Windows 10/11 x64**
- **.NET Framework 4.8** (pre-installed on Win10+)
- **Visual Studio 2022** or `dotnet build`
- Must run as **Administrator** (UAC prompt shown automatically)

## Build

```bash
# Clone / extract the solution
cd TimeMocker
dotnet restore
dotnet build -c Release -p:Platform=x64

# Outputs go to:
#   TimeMocker.UI/bin/x64/Release/net48/TimeMocker.exe
#   TimeMocker.UI/bin/x64/Release/net48/TimeMocker.Hook.dll   ← must be next to .exe
```

> In Visual Studio: open `TimeMocker.sln`, set platform to **x64**, build solution.

## Usage

### Manual Injection

1. Launch `TimeMocker.exe` (UAC will prompt for elevation)
2. Set the desired date/time in the **Mock Time Settings** bar at the top
3. Click **Set** to apply the time to all injected processes
4. Go to the **Processes** tab
5. Find your target process in the list (use Search if needed)
6. Check the **Injected** checkbox next to the process
7. The target process now sees your fake time immediately
8. To stop mocking for a process, simply uncheck the **Injected** checkbox

### Auto-Inject Rules

The auto-inject watcher starts automatically when TimeMocker launches. Any process matching a rule will be injected automatically.

1. Go to the **Auto-Inject Rules** tab
2. Enter a pattern matching the process path or name, e.g.:
   - Glob: `C:\Games\MyGame\*`
   - Glob by name: `*chrome*`
   - Regex: `^.*\\MyApp\.exe$`
3. Select **Glob** or **Regex** pattern type
4. Click **+ Add Rule**
5. Any process that starts (or is already running) matching the rule gets injected automatically with the current mock time

### Time Flow

TimeMocker uses a **delta-based** approach. When you set a fake time, it calculates the offset between your desired time and the current real time. This offset is stored and applied continuously, so the fake time flows forward naturally at the same rate as real time.

- Click **Now** to reset the date/time pickers to current time
- Click **Set** to apply the selected time to all injected processes
- The offset is recalculated each time you click **Set**

### Process List

- Only user processes are shown (system processes are filtered out)
- Click **⟳ Refresh** to reload the process list
- Dead processes are automatically removed from the injected processes list

## IPC Design

The time offset is stored in a **named Memory-Mapped File** (one per injected process):

```
Name: TimeMocker_<PID>
Size: 8 bytes
  [0..7]  DeltaTicks (Int64 — offset from DateTime.UtcNow.Ticks)
```

The hook reads this on every time API call and returns `DateTime.UtcNow + DeltaTicks`. This design allows the fake time to flow naturally without requiring timer-based updates from the UI.

## Notes & Limitations

- **64-bit only** — 32-bit processes require a separate 32-bit hook DLL build
- Processes using `QueryPerformanceCounter` for *monotonic* timing are not affected
  (QPC is a hardware register; patching it is unsupported by EasyHook)
- Anti-cheat or heavily protected processes (Epic, BattlEye, etc.) will block injection
- Some .NET apps read time through the CLR, which internally calls the hooked APIs — these *will* be affected
- EasyHook does not fully support hot-eject; to restore real time, uncheck the **Injected** checkbox
- Mocked time is always enabled — there is no passthrough mode

## License

MIT
