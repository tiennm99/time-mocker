using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Text.RegularExpressions;
using System.Threading;

namespace TimeMocker.UI.Core
{
    public class PatternRule
    {
        public string Pattern { get; set; } // glob or regex
        public bool UseRegex { get; set; }
        public bool Enabled { get; set; } = true;

        private Regex _compiled;

        public bool IsMatch(string path)
        {
            if (string.IsNullOrEmpty(path)) return false;

            if (UseRegex)
            {
                _compiled ??= new Regex(Pattern, RegexOptions.IgnoreCase);
                return _compiled.IsMatch(path);
            }

            // Glob: convert * and ? to regex
            var regexStr = "^" + Regex.Escape(Pattern)
                .Replace(@"\*", ".*")
                .Replace(@"\?", ".") + "$";
            return Regex.IsMatch(path, regexStr, RegexOptions.IgnoreCase);
        }
    }

    public class ProcessWatcher : IDisposable
    {
        private readonly InjectionManager _injectionMgr;
        private readonly List<PatternRule> _rules = new List<PatternRule>();
        private readonly HashSet<int> _seen = new HashSet<int>();
        private Timer _timer;
        private bool _running;

        // Current fake time settings to apply on auto-inject
        public DateTime FakeUtc { get; set; } = DateTime.UtcNow;
        public bool MockEnabled { get; set; } = false;

        public event Action<string> LogMessage;
        public event Action<InjectedProcess> ProcessAutoInjected;

        public IReadOnlyList<PatternRule> Rules => _rules;

        public ProcessWatcher(InjectionManager mgr)
        {
            _injectionMgr = mgr;
        }

        public void AddRule(PatternRule rule)
        {
            lock (_rules)
            {
                _rules.Add(rule);
            }
        }

        public void RemoveRule(PatternRule rule)
        {
            lock (_rules)
            {
                _rules.Remove(rule);
            }
        }

        public void ClearRules()
        {
            lock (_rules)
            {
                _rules.Clear();
            }
        }

        public void Start(int pollIntervalMs = 1500)
        {
            if (_running) return;
            _running = true;
            _timer = new Timer(_ => Scan(), null, 0, pollIntervalMs);
        }

        public void Stop()
        {
            _running = false;
            _timer?.Dispose();
            _timer = null;
        }

        private void Scan()
        {
            try
            {
                var processes = Process.GetProcesses();
                lock (_rules)
                {
                    foreach (var p in processes)
                    {
                        if (_seen.Contains(p.Id)) continue;

                        var path = "";
                        try
                        {
                            path = p.MainModule?.FileName ?? "";
                        }
                        catch
                        {
                            continue;
                        }

                        foreach (var rule in _rules)
                        {
                            if (!rule.Enabled) continue;
                            if (!rule.IsMatch(path) && !rule.IsMatch(p.ProcessName)) continue;

                            _seen.Add(p.Id);
                            try
                            {
                                var entry = _injectionMgr.Inject(p);
                                _injectionMgr.SetFakeTime(p.Id, FakeUtc, MockEnabled);
                                Log($"[AutoInject] Matched rule '{rule.Pattern}' → [{p.Id}] {p.ProcessName}");
                                ProcessAutoInjected?.Invoke(entry);
                            }
                            catch (Exception ex)
                            {
                                Log($"[AutoInject] Failed on [{p.Id}] {p.ProcessName}: {ex.Message}");
                            }

                            break;
                        }
                    }
                }
            }
            catch
            {
                /* scan errors are non-fatal */
            }
        }

        private void Log(string msg)
        {
            LogMessage?.Invoke(msg);
        }

        public void Dispose()
        {
            Stop();
        }
    }
}