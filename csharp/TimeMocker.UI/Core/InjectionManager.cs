using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using EasyHook;

namespace TimeMocker.UI.Core
{
    public class InjectedProcess
    {
        public int ProcessId { get; set; }
        public string ProcessName { get; set; }
        public string ProcessPath { get; set; }
        public SharedMemoryManager Shm { get; set; }
        public bool IsInjected { get; set; }
    }

    public class InjectionManager : IDisposable
    {
        private readonly Dictionary<int, InjectedProcess> _injected
            = new Dictionary<int, InjectedProcess>();

        private static readonly string HookDllPathX64 =
            Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "TimeMocker.Hook.x64.dll");

        private static readonly string HookDllPathX86 =
            Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "TimeMocker.Hook.x86.dll");

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool IsWow64Process(IntPtr hProcess, out bool wow64Process);

        public event Action<string> LogMessage;

        // -----------------------------------------------------------------------
        // Inject into a specific process
        // -----------------------------------------------------------------------
        public InjectedProcess Inject(Process process)
        {
            if (_injected.ContainsKey(process.Id))
                return _injected[process.Id];

            var entry = new InjectedProcess
            {
                ProcessId = process.Id,
                ProcessName = process.ProcessName,
                ProcessPath = TryGetPath(process),
                Shm = new SharedMemoryManager(process.Id)
            };

            try
            {
                // Write initial delta (zero = real time) before hook starts reading
                entry.Shm.Write(new MockTimeInfo { DeltaTicks = 0 });

                // Determine which DLL to use based on target process architecture
                string hookDllPath = GetHookDllPath(process);

                RemoteHooking.Inject(
                    process.Id,
                    InjectionOptions.DoNotRequireStrongName,
                    hookDllPath,
                    hookDllPath,
                    entry.Shm.MmfName);

                entry.IsInjected = true;
                _injected[process.Id] = entry;
                Log($"Injected into [{process.Id}] {process.ProcessName} ({GetArchitectureName(process)})");
            }
            catch (Exception ex)
            {
                entry.Shm.Dispose();
                Log($"Failed to inject into [{process.Id}] {process.ProcessName}: {ex.Message}");
                throw;
            }

            return entry;
        }

        private static string GetHookDllPath(Process process)
        {
            bool is64BitTarget = Is64BitProcess(process);

            if (is64BitTarget)
            {
                if (!File.Exists(HookDllPathX64))
                    throw new FileNotFoundException($"x64 hook DLL not found: {HookDllPathX64}");
                return HookDllPathX64;
            }
            else
            {
                if (!File.Exists(HookDllPathX86))
                    throw new FileNotFoundException($"x86 hook DLL not found: {HookDllPathX86}");
                return HookDllPathX86;
            }
        }

        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool IsWow64Process2(
            IntPtr hProcess,
            out ushort pProcessMachine,
            out ushort pNativeMachine);

        private static bool Is64BitProcess(Process process)
        {
            if (!Environment.Is64BitOperatingSystem)
                return false;

            ushort processMachine, nativeMachine;
            if (IsWow64Process2(process.Handle, out processMachine, out nativeMachine))
            {
                // IMAGE_FILE_MACHINE_UNKNOWN (0) means it's a native 64-bit process
                // IMAGE_FILE_MACHINE_I386 (0x014c) means it's 32-bit on 64-bit Windows
                return processMachine == 0x0000; // 0 = native, not emulated
            }

            // Fallback
            bool isWow64;
            IsWow64Process(process.Handle, out isWow64);
            return !isWow64;
        }

        private static string GetArchitectureName(Process process)
        {
            return Is64BitProcess(process) ? "x64" : "x86";
        }

        // -----------------------------------------------------------------------
        // Update fake time for a process
        // -----------------------------------------------------------------------
        public void SetFakeTime(int processId, DateTime fakeUtc)
        {
            if (!_injected.TryGetValue(processId, out var entry)) return;

            // Calculate delta: (desired fake time) - (current real UTC time)
            long deltaTicks = fakeUtc.Ticks - DateTime.UtcNow.Ticks;

            entry.Shm.Write(new MockTimeInfo { DeltaTicks = deltaTicks });
        }

        public void SetFakeTimeAll(DateTime fakeUtc)
        {
            foreach (var pid in _injected.Keys)
                SetFakeTime(pid, fakeUtc);
        }

        public bool IsInjected(int processId)
        {
            return _injected.ContainsKey(processId);
        }

        public IEnumerable<InjectedProcess> InjectedProcesses => _injected.Values;

        // -----------------------------------------------------------------------
        // Eject (best-effort – EasyHook doesn't fully support unloading)
        // -----------------------------------------------------------------------
        public void Eject(int processId)
        {
            if (!_injected.TryGetValue(processId, out var entry)) return;
            entry.Shm.Dispose();
            _injected.Remove(processId);
            Log($"Ejected from [{processId}] {entry.ProcessName}");
        }

        private static string TryGetPath(Process p)
        {
            try
            {
                return p.MainModule?.FileName ?? "";
            }
            catch
            {
                return "";
            }
        }

        private void Log(string msg)
        {
            LogMessage?.Invoke(msg);
        }

        public void Dispose()
        {
            foreach (var e in _injected.Values) e.Shm?.Dispose();
            _injected.Clear();
        }
    }
}