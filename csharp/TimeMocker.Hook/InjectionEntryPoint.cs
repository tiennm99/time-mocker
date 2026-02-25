using System;
using System.IO;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Threading;
using EasyHook;

namespace TimeMocker.Hook
{
    /// <summary>
    /// Shared memory layout written by the UI and read by the hook.
    /// Stored in a named memory-mapped file so no pipe latency on hot path.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct MockTimeInfo
    {
        public long FakeUtcTicks; // DateTime ticks (UTC)
        public int Enabled; // 1 = mock active, 0 = passthrough
    }

    // -------------------------------------------------------------------------
    // Win32 structs
    // -------------------------------------------------------------------------
    [StructLayout(LayoutKind.Sequential)]
    public struct SYSTEMTIME
    {
        public ushort wYear, wMonth, wDayOfWeek, wDay;
        public ushort wHour, wMinute, wSecond, wMilliseconds;

        public static SYSTEMTIME FromDateTime(DateTime dt)
        {
            return new SYSTEMTIME
            {
                wYear = (ushort)dt.Year,
                wMonth = (ushort)dt.Month,
                wDayOfWeek = (ushort)dt.DayOfWeek,
                wDay = (ushort)dt.Day,
                wHour = (ushort)dt.Hour,
                wMinute = (ushort)dt.Minute,
                wSecond = (ushort)dt.Second,
                wMilliseconds = (ushort)dt.Millisecond
            };
        }
    }

    // -------------------------------------------------------------------------
    // EasyHook entry point – called after DLL is injected
    // -------------------------------------------------------------------------
    public class InjectionEntryPoint : IEntryPoint
    {
        private readonly string _mmfName;
        private System.IO.MemoryMappedFiles.MemoryMappedFile _mmf;
        private System.IO.MemoryMappedFiles.MemoryMappedViewAccessor _view;

        // Hook handles
        private LocalHook _getSystemTimeHook;
        private LocalHook _getLocalTimeHook;
        private LocalHook _ntQuerySystemTimeHook;
        private LocalHook _getSystemTimeAsFileTimeHook;
        private LocalHook _getSystemTimePreciseAsFileTimeHook;

        public InjectionEntryPoint(RemoteHooking.IContext context, string mmfName)
        {
            _mmfName = mmfName;
        }

        public void Run(RemoteHooking.IContext context, string mmfName)
        {
            try
            {
                // Open the shared memory created by the UI process
                _mmf = System.IO.MemoryMappedFiles.MemoryMappedFile.OpenExisting(mmfName);
                _view = _mmf.CreateViewAccessor(0, Marshal.SizeOf<MockTimeInfo>());

                InstallHooks();
                RemoteHooking.WakeUpProcess();

                // Keep alive until process exits
                while (true) Thread.Sleep(500);
            }
            catch (Exception ex)
            {
                File.AppendAllText(
                    Path.Combine(Path.GetTempPath(), "TimeMocker.Hook.log"),
                    $"[{DateTime.Now}] ERROR: {ex}\r\n");
            }
            finally
            {
                _getSystemTimeHook?.Dispose();
                _getLocalTimeHook?.Dispose();
                _ntQuerySystemTimeHook?.Dispose();
                _getSystemTimeAsFileTimeHook?.Dispose();
                _getSystemTimePreciseAsFileTimeHook?.Dispose();
                _view?.Dispose();
                _mmf?.Dispose();
            }
        }

        // -------------------------------------------------------------------------
        // Helpers
        // -------------------------------------------------------------------------
        private MockTimeInfo ReadMockInfo()
        {
            _view.Read(0, out MockTimeInfo info);
            return info;
        }

        private DateTime GetFakeUtc()
        {
            var info = ReadMockInfo();
            return info.Enabled == 1
                ? new DateTime(info.FakeUtcTicks, DateTimeKind.Utc)
                : DateTime.UtcNow;
        }

        private void InstallHooks()
        {
            _getSystemTimeHook = LocalHook.Create(
                LocalHook.GetProcAddress("kernel32.dll", "GetSystemTime"),
                new GetSystemTimeDelegate(GetSystemTime_Hook), this);
            _getSystemTimeHook.ThreadACL.SetExclusiveACL(new[] { 0 });

            _getLocalTimeHook = LocalHook.Create(
                LocalHook.GetProcAddress("kernel32.dll", "GetLocalTime"),
                new GetLocalTimeDelegate(GetLocalTime_Hook), this);
            _getLocalTimeHook.ThreadACL.SetExclusiveACL(new[] { 0 });

            _ntQuerySystemTimeHook = LocalHook.Create(
                LocalHook.GetProcAddress("ntdll.dll", "NtQuerySystemTime"),
                new NtQuerySystemTimeDelegate(NtQuerySystemTime_Hook), this);
            _ntQuerySystemTimeHook.ThreadACL.SetExclusiveACL(new[] { 0 });

            _getSystemTimeAsFileTimeHook = LocalHook.Create(
                LocalHook.GetProcAddress("kernel32.dll", "GetSystemTimeAsFileTime"),
                new GetSystemTimeAsFileTimeDelegate(GetSystemTimeAsFileTime_Hook), this);
            _getSystemTimeAsFileTimeHook.ThreadACL.SetExclusiveACL(new[] { 0 });

            _getSystemTimePreciseAsFileTimeHook = LocalHook.Create(
                LocalHook.GetProcAddress("kernel32.dll", "GetSystemTimePreciseAsFileTime"),
                new GetSystemTimeAsFileTimeDelegate(GetSystemTimePreciseAsFileTime_Hook), this);
            _getSystemTimePreciseAsFileTimeHook.ThreadACL.SetExclusiveACL(new[] { 0 });
        }

        // -------------------------------------------------------------------------
        // Hook implementations
        // -------------------------------------------------------------------------

        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        private delegate void GetSystemTimeDelegate(out SYSTEMTIME lpSystemTime);

        private void GetSystemTime_Hook(out SYSTEMTIME lpSystemTime)
        {
            lpSystemTime = SYSTEMTIME.FromDateTime(GetFakeUtc());
        }

        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        private delegate void GetLocalTimeDelegate(out SYSTEMTIME lpLocalTime);

        private void GetLocalTime_Hook(out SYSTEMTIME lpLocalTime)
        {
            lpLocalTime = SYSTEMTIME.FromDateTime(GetFakeUtc().ToLocalTime());
        }

        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        private delegate int NtQuerySystemTimeDelegate(out long systemTime);

        private int NtQuerySystemTime_Hook(out long systemTime)
        {
            // FILETIME epoch: Jan 1, 1601
            var epoch = new DateTime(1601, 1, 1, 0, 0, 0, DateTimeKind.Utc);
            systemTime = (GetFakeUtc() - epoch).Ticks;
            return 0; // STATUS_SUCCESS
        }

        [UnmanagedFunctionPointer(CallingConvention.StdCall)]
        private delegate void GetSystemTimeAsFileTimeDelegate(out long lpFileTime);

        private void GetSystemTimeAsFileTime_Hook(out long lpFileTime)
        {
            var epoch = new DateTime(1601, 1, 1, 0, 0, 0, DateTimeKind.Utc);
            lpFileTime = (GetFakeUtc() - epoch).Ticks;
        }

        private void GetSystemTimePreciseAsFileTime_Hook(out long lpFileTime)
        {
            var epoch = new DateTime(1601, 1, 1, 0, 0, 0, DateTimeKind.Utc);
            lpFileTime = (GetFakeUtc() - epoch).Ticks;
        }
    }
}