using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Windows.Forms;
using TimeMocker.UI.Core;

namespace TimeMocker.UI.Forms
{
    public class PatternRuleDto
    {
        public string Pattern { get; set; }
        public bool UseRegex { get; set; }
        public bool Enabled { get; set; } = true;
    }

    public class AppConfig
    {
        private static readonly string ConfigPath = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "TimeMocker",
            "timemocker-config.json");

        public List<PatternRuleDto> Patterns { get; set; } = new List<PatternRuleDto>();

        public void Save()
        {
            var dir = Path.GetDirectoryName(ConfigPath);
            if (!string.IsNullOrEmpty(dir) && !Directory.Exists(dir))
            {
                Directory.CreateDirectory(dir);
            }
            File.WriteAllText(ConfigPath, JsonSerializer.Serialize(this, new JsonSerializerOptions { WriteIndented = true }));
        }

        public static AppConfig Load()
        {
            return File.Exists(ConfigPath)
                ? JsonSerializer.Deserialize<AppConfig>(File.ReadAllText(ConfigPath)) ?? new AppConfig()
                : new AppConfig();
        }
    }

    public partial class MainForm : Form
    {
        private readonly AppConfig _config;

        private InjectionManager _injMgr;
        private ProcessWatcher _watcher;

        // Controls
        private TabControl tabMain;
        private TabPage tabProcesses, tabPatterns, tabLog;

        // -- Process tab
        private DataGridView dgvProcesses;
        private Button btnRefresh;
        private TextBox txtProcSearch;
        private Label lblProcSearch;

        // -- Time panel (shared)
        private GroupBox grpTime;
        private DateTimePicker dtpDate;
        private DateTimePicker dtpTime;
        private Button btnApply;
        private Label lblPreview;

        // -- Patterns tab
        private DataGridView dgvPatterns;
        private Button btnAddPattern, btnRemovePattern;
        private TextBox txtNewPattern;
        private RadioButton rdoGlob, rdoRegex;

        // -- Log tab
        private RichTextBox rtbLog;
        private Button btnClearLog;

        public MainForm()
        {
            _config = AppConfig.Load();

            Text = "TimeMocker – Process Time Injection";
            Size = new Size(900, 680);
            MinimumSize = new Size(750, 560);
            StartPosition = FormStartPosition.CenterScreen;
            Font = new Font("Segoe UI", 9f);
            BackColor = Color.FromArgb(45, 52, 64);
            ForeColor = Color.FromArgb(200, 200, 200);

            _injMgr = new InjectionManager();
            _watcher = new ProcessWatcher(_injMgr);

            _injMgr.LogMessage += AppendLog;
            _watcher.LogMessage += AppendLog;
            _watcher.ProcessAutoInjected += entry =>
                BeginInvoke((Action)(() => RefreshProcessList()));

            BuildUI();

            // Load config settings
            foreach (var pattern in _config.Patterns)
            {
                _watcher.AddRule(new PatternRule
                {
                    Pattern = pattern.Pattern,
                    UseRegex = pattern.UseRegex,
                    Enabled = pattern.Enabled
                });
                dgvPatterns.Rows.Add(pattern.Pattern, pattern.UseRegex ? "Regex" : "Glob", pattern.Enabled);
            }

            RefreshProcessList();
            UpdateTimePreview();
            ApplyTime(); // Initialize with current time

            // Start auto-inject watcher by default
            _watcher.FakeUtc = GetFakeTime().ToUniversalTime();
            _watcher.Start();
            AppendLog("Process watcher started.");
        }

        // =====================================================================
        // UI Builder
        // =====================================================================
        private void BuildUI()
        {
            // ---- Shared time panel (top) ------------------------------------
            grpTime = new GroupBox
            {
                Text = "Mock Time Settings",
                Dock = DockStyle.Top,
                Height = 110,
                ForeColor = Color.FromArgb(100, 160, 220),
                Padding = new Padding(8)
            };

            var timeFlow = new FlowLayoutPanel
            {
                Dock = DockStyle.Fill,
                FlowDirection = FlowDirection.LeftToRight,
                WrapContents = false,
                AutoSize = false
            };

            dtpDate = new DateTimePicker
            {
                Format = DateTimePickerFormat.Short,
                Width = 120,
                Height = 26,
                Value = DateTime.Now,
                Margin = new Padding(4, 10, 4, 0)
            };
            dtpDate.ValueChanged += (s, e) => UpdateTimePreview();

            dtpTime = new DateTimePicker
            {
                Format = DateTimePickerFormat.Time,
                ShowUpDown = true,
                Width = 100,
                Height = 26,
                Value = DateTime.Now,
                Margin = new Padding(4, 10, 4, 0)
            };
            dtpTime.ValueChanged += (s, e) => UpdateTimePreview();

            var btnSetNow = MakeButton("Now", 70, Color.FromArgb(100, 110, 120));
            btnSetNow.Margin = new Padding(4, 10, 4, 0);
            btnSetNow.Click += (s, e) =>
            {
                dtpDate.Value = dtpTime.Value = DateTime.Now;
            };

            btnApply = MakeButton("Set", 70, Color.FromArgb(70, 140, 200));
            btnApply.Margin = new Padding(4, 10, 4, 0);
            btnApply.Click += (s, e) => ApplyTime();

            lblPreview = new Label
            {
                AutoSize = false,
                Width = 300,
                Height = 20,
                ForeColor = Color.FromArgb(140, 150, 160),
                Font = new Font("Segoe UI", 8.5f, FontStyle.Italic),
                Margin = new Padding(4, 14, 0, 0)
            };

            timeFlow.Controls.AddRange(new Control[]
            {
                dtpDate, dtpTime, btnSetNow, btnApply, lblPreview
            });
            grpTime.Controls.Add(timeFlow);

            // ---- Tabs -------------------------------------------------------
            tabMain = new TabControl
            {
                Dock = DockStyle.Fill,
                DrawMode = TabDrawMode.OwnerDrawFixed,
                SizeMode = TabSizeMode.Fixed,
                ItemSize = new Size(120, 28)
            };
            tabMain.DrawItem += DrawTab;

            tabProcesses = new TabPage("Processes");
            tabPatterns = new TabPage("Auto-Inject Rules");
            tabLog = new TabPage("Log");
            StyleTab(tabProcesses);
            StyleTab(tabPatterns);
            StyleTab(tabLog);

            BuildProcessTab();
            BuildPatternsTab();
            BuildLogTab();

            tabMain.TabPages.AddRange(new[] { tabProcesses, tabPatterns, tabLog });

            Controls.Add(tabMain);
            Controls.Add(grpTime);
        }

        // =====================================================================
        // Process Tab
        // =====================================================================
        private void BuildProcessTab()
        {
            var panel = new Panel { Dock = DockStyle.Fill };

            // Top toolbar
            var toolbar = new FlowLayoutPanel
            {
                Dock = DockStyle.Top,
                Height = 40,
                Padding = new Padding(4)
            };

            lblProcSearch = new Label { Text = "Search:", AutoSize = true, Margin = new Padding(4, 8, 2, 0) };
            txtProcSearch = new TextBox { Width = 180, Margin = new Padding(0, 6, 8, 0) };
            txtProcSearch.TextChanged += (s, e) => FilterProcessList();

            btnRefresh = MakeButton("⟳ Refresh", 90, Color.FromArgb(100, 110, 120));
            btnRefresh.Margin = new Padding(0, 6, 4, 0);
            btnRefresh.Click += (s, e) => RefreshProcessList();

            toolbar.Controls.AddRange(new Control[] { lblProcSearch, txtProcSearch, btnRefresh });
            toolbar.BackColor = Color.FromArgb(55, 62, 74);

            // Single process list with Inject checkbox
            var lblSection = MakeSectionLabel("Processes");
            dgvProcesses = MakeGrid();
            dgvProcesses.ReadOnly = false;
            dgvProcesses.MultiSelect = false;
            dgvProcesses.AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.None;
            dgvProcesses.Columns.AddRange(
                Col("PID", 50), Col("Name", 160), Col("Path", 380), BoolCol("Injected", 70));

            // Make all columns read-only except the checkbox
            foreach (DataGridViewColumn col in dgvProcesses.Columns)
            {
                if (col.Name != "Injected")
                    col.ReadOnly = true;
            }

            dgvProcesses.CellValueChanged += dgvProcesses_CellValueChanged;
            dgvProcesses.CurrentCellDirtyStateChanged += (s, e) =>
            {
                if (dgvProcesses.IsCurrentCellDirty)
                {
                    dgvProcesses.CommitEdit(DataGridViewDataErrorContexts.Commit);
                }
            };

            panel.Controls.Add(lblSection);
            panel.Controls.Add(dgvProcesses);
            panel.Controls.Add(toolbar);

            tabProcesses.Controls.Add(panel);
        }

        private List<ProcessRow> _allRows = new List<ProcessRow>();

        private class ProcessRow
        {
            public int Id;
            public string Name, Path;
            public bool Injected;
        }

        private void dgvProcesses_CellValueChanged(object sender, DataGridViewCellEventArgs e)
        {
            if (e.RowIndex < 0 || e.ColumnIndex < 0) return;
            if (dgvProcesses.Columns[e.ColumnIndex].Name != "Injected") return;

            var pidCell = dgvProcesses.Rows[e.RowIndex].Cells[0];
            if (pidCell.Value == null) return;

            var pid = (int)pidCell.Value;
            var shouldInject = (bool)dgvProcesses.Rows[e.RowIndex].Cells[e.ColumnIndex].Value;

            if (shouldInject)
            {
                // Inject the process
                if (!_injMgr.IsInjected(pid))
                {
                    try
                    {
                        var p = Process.GetProcessById(pid);
                        _injMgr.Inject(p);
                        var dt = GetFakeTime();
                        _injMgr.SetFakeTime(p.Id, dt.ToUniversalTime());
                        AppendLog($"Injected into [{pid}] {p.ProcessName}");
                    }
                    catch (Exception ex)
                    {
                        MessageBox.Show($"Injection failed:\n{ex.Message}", "Error",
                            MessageBoxButtons.OK, MessageBoxIcon.Error);
                        // Revert checkbox on failure
                        dgvProcesses.BeginInvoke((Action)(() =>
                        {
                            dgvProcesses.Rows[e.RowIndex].Cells[e.ColumnIndex].Value = false;
                        }));
                    }
                }
            }
            else
            {
                // Eject the process
                if (_injMgr.IsInjected(pid))
                {
                    _injMgr.Eject(pid);
                    AppendLog($"Ejected from [{pid}]");
                }
            }
        }

        private void RefreshProcessList()
        {
            _allRows.Clear();

            // Get current live process IDs
            var livePids = new HashSet<int>(Process.GetProcesses().Select(p => p.Id));

            // Remove dead processes from injection manager
            var deadPids = _injMgr.InjectedProcesses
                .Select(x => x.ProcessId)
                .Where(pid => !livePids.Contains(pid))
                .ToList();

            foreach (var deadPid in deadPids)
            {
                _injMgr.Eject(deadPid);
            }

            var injectedPids = new HashSet<int>(_injMgr.InjectedProcesses.Select(x => x.ProcessId));

            foreach (var p in Process.GetProcesses().OrderBy(x => x.ProcessName))
            {
                var path = "";
                try
                {
                    path = p.MainModule?.FileName ?? "";
                }
                catch
                {
                    // Skip processes we can't access
                    continue;
                }

                _allRows.Add(new ProcessRow
                {
                    Id = p.Id,
                    Name = p.ProcessName,
                    Path = path,
                    Injected = injectedPids.Contains(p.Id)
                });
            }

            FilterProcessList();
        }

        private void FilterProcessList()
        {
            var q = txtProcSearch.Text.Trim().ToLower();
            dgvProcesses.SuspendLayout();
            dgvProcesses.Rows.Clear();
            foreach (var r in _allRows)
            {
                if (q.Length > 0 && !r.Name.ToLower().Contains(q) && !r.Path.ToLower().Contains(q)) continue;
                dgvProcesses.Rows.Add(r.Id, r.Name, r.Path, r.Injected);
            }
            dgvProcesses.ResumeLayout();
        }

        // =====================================================================
        // Patterns Tab
        // =====================================================================
        // Patterns Tab
        // =====================================================================
        private void BuildPatternsTab()
        {
            var panel = new Panel { Dock = DockStyle.Fill };

            var toolbar = new FlowLayoutPanel
            {
                Dock = DockStyle.Top,
                Height = 80,
                Padding = new Padding(4),
                BackColor = Color.FromArgb(55, 62, 74)
            };

            // Pattern input row
            var lblNew = new Label { Text = "Pattern:", AutoSize = true, Margin = new Padding(4, 12, 4, 0) };
            txtNewPattern = new TextBox
                { Width = 280, Margin = new Padding(0, 10, 4, 0), Text = "e.g. C:\\Games\\MyGame\\* or ^.*chrome.*$" };

            rdoGlob = new RadioButton
            {
                Text = "Glob", Checked = true, AutoSize = true, Margin = new Padding(4, 12, 4, 0),
                ForeColor = Color.FromArgb(200, 200, 200)
            };
            rdoRegex = new RadioButton
            {
                Text = "Regex", AutoSize = true, Margin = new Padding(4, 12, 4, 0),
                ForeColor = Color.FromArgb(200, 200, 200)
            };

            btnAddPattern = MakeButton("+ Add Rule", 100, Color.FromArgb(80, 160, 100));
            btnAddPattern.Margin = new Padding(8, 8, 4, 0);
            btnAddPattern.Click += OnAddPattern;

            btnRemovePattern = MakeButton("✕ Remove", 90, Color.FromArgb(190, 90, 90));
            btnRemovePattern.Margin = new Padding(4, 8, 4, 0);
            btnRemovePattern.Click += OnRemovePattern;

            toolbar.Controls.AddRange(new Control[]
            {
                lblNew, txtNewPattern, rdoGlob, rdoRegex,
                btnAddPattern, btnRemovePattern
            });

            var lblSection = MakeSectionLabel("Auto-Inject Rules (process path or name must match)");

            dgvPatterns = MakeGrid();
            dgvPatterns.Columns.AddRange(
                Col("Pattern", 320), Col("Type", 60), BoolCol("Enabled"));

            panel.Controls.Add(dgvPatterns);
            panel.Controls.Add(lblSection);
            panel.Controls.Add(toolbar);

            tabPatterns.Controls.Add(panel);
        }

        private void OnAddPattern(object sender, EventArgs e)
        {
            var pat = txtNewPattern.Text.Trim();
            if (string.IsNullOrEmpty(pat)) return;

            var rule = new PatternRule
            {
                Pattern = pat,
                UseRegex = rdoRegex.Checked,
                Enabled = true
            };
            _watcher.AddRule(rule);
            dgvPatterns.Rows.Add(pat, rule.UseRegex ? "Regex" : "Glob", true);
            txtNewPattern.Clear();
        }

        private void OnRemovePattern(object sender, EventArgs e)
        {
            if (dgvPatterns.SelectedRows.Count == 0) return;
            var idx = dgvPatterns.SelectedRows[0].Index;
            var pat = dgvPatterns.Rows[idx].Cells[0].Value?.ToString();
            _watcher.ClearRules();
            dgvPatterns.Rows.RemoveAt(idx);
            // Re-add remaining
            foreach (DataGridViewRow row in dgvPatterns.Rows)
                _watcher.AddRule(new PatternRule
                {
                    Pattern = row.Cells[0].Value?.ToString() ?? "",
                    UseRegex = row.Cells[1].Value?.ToString() == "Regex",
                    Enabled = (bool)(row.Cells[2].Value ?? true)
                });
        }

        // =====================================================================
        // Log Tab
        // =====================================================================
        private void BuildLogTab()
        {
            rtbLog = new RichTextBox
            {
                Dock = DockStyle.Fill,
                BackColor = Color.FromArgb(30, 35, 42),
                ForeColor = Color.FromArgb(170, 210, 150),
                Font = new Font("Consolas", 9f),
                ReadOnly = true,
                ScrollBars = RichTextBoxScrollBars.Vertical
            };

            btnClearLog = MakeButton("Clear", 70, Color.FromArgb(100, 110, 120));
            btnClearLog.Dock = DockStyle.Bottom;
            btnClearLog.Click += (s, e) => rtbLog.Clear();

            tabLog.Controls.Add(rtbLog);
            tabLog.Controls.Add(btnClearLog);
        }

        // =====================================================================
        // Time Logic
        // =====================================================================
        private DateTime GetFakeTime()
        {
            return dtpDate.Value.Date + dtpTime.Value.TimeOfDay;
        }

        private void ApplyTime()
        {
            var dt = GetFakeTime().ToUniversalTime();
            _injMgr.SetFakeTimeAll(dt);
            _watcher.FakeUtc = dt;

            UpdateTimePreview();
        }

        private void UpdateTimePreview()
        {
            var dt = GetFakeTime();
            // Calculate what the current offset is
            var realNow = DateTime.Now;
            var delta = dt - realNow;
            var deltaStr = delta.TotalSeconds >= 0
                ? $"+{delta.TotalSeconds:F0}s"
                : $"{delta.TotalSeconds:F0}s";
            lblPreview.Text = $"Fake: {dt:yyyy-MM-dd HH:mm:ss} (local, offset {deltaStr})";
        }

        // =====================================================================
        // Helpers
        // =====================================================================
        private void AppendLog(string msg)
        {
            if (rtbLog == null) return;
            if (rtbLog.InvokeRequired)
            {
                rtbLog.BeginInvoke((Action)(() => AppendLog(msg)));
                return;
            }

            rtbLog.AppendText($"[{DateTime.Now:HH:mm:ss}] {msg}\n");
            rtbLog.ScrollToCaret();
        }

        private static DataGridView MakeGrid()
        {
            var g = new DataGridView
            {
                Dock = DockStyle.Fill,
                ReadOnly = true,
                AllowUserToAddRows = false,
                AllowUserToDeleteRows = false,
                SelectionMode = DataGridViewSelectionMode.FullRowSelect,
                MultiSelect = false,
                BackgroundColor = Color.FromArgb(52, 58, 68),
                ForeColor = Color.FromArgb(175, 180, 185),
                GridColor = Color.FromArgb(85, 92, 102),
                BorderStyle = BorderStyle.None,
                RowHeadersVisible = false,
                AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.DisplayedCells,
                ColumnHeadersHeightSizeMode = DataGridViewColumnHeadersHeightSizeMode.AutoSize,
                ColumnHeadersHeight = 28
            };
            g.EnableHeadersVisualStyles = false;
            g.ColumnHeadersDefaultCellStyle.BackColor = Color.FromArgb(72, 78, 88);
            g.ColumnHeadersDefaultCellStyle.ForeColor = Color.FromArgb(160, 185, 210);
            g.DefaultCellStyle.SelectionBackColor = Color.FromArgb(98, 138, 178);
            g.AlternatingRowsDefaultCellStyle.BackColor = Color.FromArgb(58, 64, 74);
            return g;
        }

        private static DataGridViewTextBoxColumn Col(string name, int w)
        {
            return new DataGridViewTextBoxColumn
            {
                Name = name,
                HeaderText = name,
                Width = w,
                MinimumWidth = w,
                SortMode = DataGridViewColumnSortMode.NotSortable
            };
        }

        private static DataGridViewCheckBoxColumn BoolCol(string name, int width = 65)
        {
            return new DataGridViewCheckBoxColumn
            {
                Name = name,
                HeaderText = name,
                Width = width,
                MinimumWidth = width
            };
        }

        private static Button MakeButton(string text, int w, Color bg)
        {
            return new Button
            {
                Text = text,
                Width = w,
                Height = 26,
                BackColor = bg,
                ForeColor = Color.White,
                FlatStyle = FlatStyle.Flat,
                Cursor = Cursors.Hand
            };
        }

        private static Label MakeSectionLabel(string text)
        {
            return new Label
            {
                Text = text,
                Dock = DockStyle.Top,
                Height = 22,
                ForeColor = Color.FromArgb(150, 180, 220),
                Font = new Font("Segoe UI", 8.5f, FontStyle.Bold),
                Padding = new Padding(4, 2, 0, 0),
                BackColor = Color.FromArgb(50, 57, 69)
            };
        }

        private static void StyleTab(TabPage tab)
        {
            tab.BackColor = Color.FromArgb(45, 52, 64);
            tab.ForeColor = Color.FromArgb(200, 200, 200);
        }

        private void DrawTab(object sender, DrawItemEventArgs e)
        {
            var tab = (TabControl)sender;
            var page = tab.TabPages[e.Index];
            var rect = e.Bounds;
            var selected = e.Index == tab.SelectedIndex;

            using var bg = new SolidBrush(selected ? Color.FromArgb(80, 130, 180) : Color.FromArgb(60, 68, 80));
            e.Graphics.FillRectangle(bg, rect);

            var sf = new StringFormat
                { Alignment = StringAlignment.Center, LineAlignment = StringAlignment.Center };
            using var fg = new SolidBrush(selected ? Color.White : Color.FromArgb(170, 180, 190));
            e.Graphics.DrawString(page.Text, Font, fg, rect, sf);
        }

        private static void ShowInfo(string msg)
        {
            MessageBox.Show(msg, "Info", MessageBoxButtons.OK, MessageBoxIcon.Information);
        }

        protected override void OnFormClosing(FormClosingEventArgs e)
        {
            // Save config
            _config.Patterns.Clear();
            foreach (DataGridViewRow row in dgvPatterns.Rows)
            {
                _config.Patterns.Add(new PatternRuleDto
                {
                    Pattern = row.Cells[0].Value?.ToString() ?? "",
                    UseRegex = row.Cells[1].Value?.ToString() == "Regex",
                    Enabled = (bool)(row.Cells[2].Value ?? true)
                });
            }
            _config.Save();

            _watcher.Stop();
            _injMgr.Dispose();
            base.OnFormClosing(e);
        }
    }
}