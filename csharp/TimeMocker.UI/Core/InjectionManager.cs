using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
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

                RemoteHooking.Inject(
                    process.Id,
                    InjectionOptions.DoNotRequireStrongName,
                    HookDllPathX86,
                    HookDllPathX64,
                    entry.Shm.MmfName);

                entry.IsInjected = true;
                _injected[process.Id] = entry;
                Log($"Injected into [{process.Id}] {process.ProcessName}");
            }
            catch (Exception ex)
            {
                entry.Shm.Dispose();
                Log($"Failed to inject into [{process.Id}] {process.ProcessName}: {ex.Message}");
                throw;
            }

            return entry;
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