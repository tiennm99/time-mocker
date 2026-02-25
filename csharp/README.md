# TimeMocker

A Windows application that injects fake time into running processes by hooking Win32 time APIs.

## Architecture

```
TimeMocker.sln
├── TimeMocker.UI        — WinForms controller app (run as Admin)
│   ├── Forms/MainForm   — UI: process chooser, time picker, pattern manager
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
2. **Processes tab** → search for your target process → select it
3. Set the desired date/time in the **Mock Time Settings** bar at the top
4. Tick **Enable Mock** → click **Inject →**
5. The target process now sees your fake time immediately

### Auto-Inject Rules

1. Go to the **Auto-Inject Rules** tab
2. Enter a pattern matching the process path or name, e.g.:
   - Glob: `C:\Games\MyGame\*`
   - Glob by name: `*chrome*`
   - Regex: `^.*\\MyApp\.exe$`
3. Click **+ Add Rule**
4. Tick **Enable Auto-Inject Watcher**
5. Any process that starts (or is already running) matching the rule gets injected automatically

### Auto-Advance Time

Tick **Auto-advance time** in the time bar — the fake time ticks forward in sync with real time from the moment you set it.

## IPC Design

The fake time is stored in a **named Memory-Mapped File** (one per injected process):

```
Name: TimeMocker_<PID>
Size: 12 bytes
  [0..7]  FakeUtcTicks (Int64 — DateTime.Ticks)
  [8..11] Enabled      (Int32 — 0=passthrough, 1=mock)
```

The hook reads this on every time API call (~50 ns read, no syscall). The UI writes it whenever you click **Apply** or toggle the checkbox.

## Notes & Limitations

- **64-bit only** — 32-bit processes require a separate 32-bit hook DLL build
- Processes using `QueryPerformanceCounter` for *monotonic* timing are not affected  
  (QPC is a hardware register; patching it is unsupported by EasyHook)
- Anti-cheat or heavily protected processes (Epic, BattlEye, etc.) will block injection
- Some .NET apps read time through the CLR, which internally calls the hooked APIs — these *will* be affected
- EasyHook does not fully support hot-eject; to restore real time, disable the mock via the checkbox rather than ejecting

## License

MIT
