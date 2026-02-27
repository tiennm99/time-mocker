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

        private static readonly string HookDllPath =
            Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "TimeMocker.Hook.dll");

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
                // Write disabled state initially so hook passes through
                entry.Shm.Write(new MockTimeInfo { Enabled = 0, DeltaTicks = 0 });

                RemoteHooking.Inject(
                    process.Id,
                    InjectionOptions.DoNotRequireStrongName,
                    HookDllPath,
                    HookDllPath,
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
        public void SetFakeTime(int processId, DateTime fakeUtc, bool enabled)
        {
            if (!_injected.TryGetValue(processId, out var entry)) return;

            long deltaTicks;
            if (enabled)
            {
                // Calculate delta: (desired fake time) - (current real UTC time)
                deltaTicks = fakeUtc.Ticks - DateTime.UtcNow.Ticks;
            }
            else
            {
                deltaTicks = 0;
            }

            entry.Shm.Write(new MockTimeInfo
            {
                DeltaTicks = deltaTicks,
                Enabled = enabled ? 1 : 0
            });
        }

        public void SetFakeTimeAll(DateTime fakeUtc, bool enabled)
        {
            foreach (var pid in _injected.Keys)
                SetFakeTime(pid, fakeUtc, enabled);
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
            entry.Shm.Write(new MockTimeInfo { Enabled = 0, DeltaTicks = 0 }); // disable mock first
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