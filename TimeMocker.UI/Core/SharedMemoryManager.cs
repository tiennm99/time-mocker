using System;
using System.IO.MemoryMappedFiles;
using System.Runtime.InteropServices;

namespace TimeMocker.UI.Core
{
    /// <summary>
    /// Creates a named Memory-Mapped File so the injected hook can read
    /// the fake time without any IPC latency on the hot path.
    /// One SharedMemoryManager per injected process.
    /// </summary>
    public class SharedMemoryManager : IDisposable
    {
        public const string MmfPrefix = "TimeMocker_";
        private MemoryMappedFile _mmf;
        private MemoryMappedViewAccessor _view;
        private readonly int _size;
        private bool _disposed;

        public string MmfName { get; }

        public SharedMemoryManager(int processId)
        {
            MmfName = MmfPrefix + processId;
            _size   = Marshal.SizeOf<MockTimeInfo>();
            _mmf    = MemoryMappedFile.CreateOrOpen(MmfName, _size,
                          MemoryMappedFileAccess.ReadWrite);
            _view   = _mmf.CreateViewAccessor(0, _size);
        }

        public void Write(MockTimeInfo info)
        {
            _view.Write(0, ref info);
            _view.Flush();
        }

        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;
            _view?.Dispose();
            _mmf?.Dispose();
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct MockTimeInfo
    {
        public long FakeUtcTicks;
        public int  Enabled;
    }
}
