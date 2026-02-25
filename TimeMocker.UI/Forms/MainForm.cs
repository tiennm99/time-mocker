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

        public bool AutoInjectEnabled { get; set; } = true;
        public bool AutoAdvanceEnabled { get; set; } = true;
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
        private Button btnRefresh, btnInject, btnEject;
        private TextBox txtProcSearch;
        private Label lblProcSearch;

        // -- Time panel (shared)
        private GroupBox grpTime;
        private DateTimePicker dtpDate;
        private DateTimePicker dtpTime;
        private CheckBox chkMockEnabled;
        private Button btnApply;
        private Label lblPreview;
        private CheckBox chkAutoAdvance;

        // -- Patterns tab
        private DataGridView dgvPatterns;
        private Button btnAddPattern, btnRemovePattern;
        private CheckBox chkWatcherEnabled;
        private TextBox txtNewPattern;
        private RadioButton rdoGlob, rdoRegex;

        // -- Log tab
        private RichTextBox rtbLog;
        private Button btnClearLog;

        // Time advancing
        private Timer _advanceTimer;
        private DateTime _fakeTimeBase;
        private DateTime _advanceStartReal;

        public MainForm()
        {
            _config = AppConfig.Load();

            Text = "TimeMocker – Process Time Injection";
            Size = new Size(900, 680);
            MinimumSize = new Size(750, 560);
            StartPosition = FormStartPosition.CenterScreen;
            Font = new Font("Segoe UI", 9f);
            BackColor = Color.FromArgb(30, 30, 35);
            ForeColor = Color.FromArgb(220, 220, 220);

            _injMgr = new InjectionManager();
            _watcher = new ProcessWatcher(_injMgr);

            _injMgr.LogMessage += AppendLog;
            _watcher.LogMessage += AppendLog;
            _watcher.ProcessAutoInjected += entry =>
                BeginInvoke((Action)(() => RefreshInjectedTab()));

            BuildUI();

            // Load config settings
            chkWatcherEnabled.Checked = _config.AutoInjectEnabled;
            chkAutoAdvance.Checked = _config.AutoAdvanceEnabled;
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
                ForeColor = Color.FromArgb(130, 200, 255),
                Padding = new Padding(8)
            };

            var timeFlow = new FlowLayoutPanel
            {
                Dock = DockStyle.Fill,
                FlowDirection = FlowDirection.LeftToRight,
                WrapContents = false,
                AutoSize = false
            };

            chkMockEnabled = new CheckBox
            {
                Text = "Enable Mock",
                ForeColor = Color.LightGreen,
                Width = 110,
                Height = 30,
                Margin = new Padding(4, 12, 4, 0)
            };
            chkMockEnabled.CheckedChanged += (s, e) => ApplyTime();

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

            btnApply = MakeButton("Apply to All", 100, Color.FromArgb(0, 120, 215));
            btnApply.Margin = new Padding(8, 10, 4, 0);
            btnApply.Click += (s, e) => ApplyTime();

            var btnSetNow = MakeButton("Set to Now", 90, Color.FromArgb(60, 60, 70));
            btnSetNow.Margin = new Padding(4, 10, 4, 0);
            btnSetNow.Click += (s, e) =>
            {
                dtpDate.Value = dtpTime.Value = DateTime.Now;
                ApplyTime();
            };

            chkAutoAdvance = new CheckBox
            {
                Text = "Auto-advance time",
                ForeColor = Color.FromArgb(220, 220, 220),
                Width = 140,
                Height = 30,
                Margin = new Padding(8, 12, 4, 0),
                Checked = true
            };
            chkAutoAdvance.CheckedChanged += ToggleAutoAdvance;

            lblPreview = new Label
            {
                AutoSize = false,
                Width = 260,
                Height = 20,
                ForeColor = Color.FromArgb(180, 180, 180),
                Font = new Font("Segoe UI", 8.5f, FontStyle.Italic),
                Margin = new Padding(4, 14, 0, 0)
            };

            timeFlow.Controls.AddRange(new Control[]
            {
                chkMockEnabled, dtpDate, dtpTime, btnApply, btnSetNow,
                chkAutoAdvance, lblPreview
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

            // advance timer
            _advanceTimer = new Timer { Interval = 1000 };
            _advanceTimer.Tick += AdvanceTick;
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

            btnRefresh = MakeButton("⟳ Refresh", 90, Color.FromArgb(60, 60, 70));
            btnRefresh.Margin = new Padding(0, 6, 4, 0);
            btnRefresh.Click += (s, e) => RefreshProcessList();

            btnInject = MakeButton("Inject →", 90, Color.FromArgb(0, 150, 80));
            btnInject.Margin = new Padding(0, 6, 4, 0);
            btnInject.Click += OnInjectClick;

            btnEject = MakeButton("✕ Eject", 80, Color.FromArgb(180, 40, 40));
            btnEject.Margin = new Padding(0, 6, 4, 0);
            btnEject.Click += OnEjectClick;

            toolbar.Controls.AddRange(new Control[] { lblProcSearch, txtProcSearch, btnRefresh, btnInject, btnEject });
            toolbar.BackColor = Color.FromArgb(30, 30, 35);

            // Grid – split: available processes | injected processes
            var split = new SplitContainer
            {
                Dock = DockStyle.Fill,
                Orientation = Orientation.Horizontal,
                SplitterDistance = 300,
                SplitterWidth = 5,
                BackColor = Color.FromArgb(50, 50, 55)
            };

            // Top: available
            var lblAvail = MakeSectionLabel("Running Processes");
            dgvProcesses = MakeGrid();
            dgvProcesses.Columns.AddRange(
                Col("PID", 50), Col("Name", 160), Col("Path", 380));

            var topPanel = new Panel { Dock = DockStyle.Fill };
            topPanel.Controls.Add(dgvProcesses);
            topPanel.Controls.Add(lblAvail);

            // Bottom: injected
            var lblInj = MakeSectionLabel("Injected Processes");
            var dgvInjected = MakeGrid();
            dgvInjected.Tag = "injected";
            dgvInjected.Columns.AddRange(
                Col("PID", 50), Col("Name", 160), Col("Path", 380), Col("Status", 80));

            var botPanel = new Panel { Dock = DockStyle.Fill };
            botPanel.Controls.Add(dgvInjected);
            botPanel.Controls.Add(lblInj);

            split.Panel1.Controls.Add(topPanel);
            split.Panel2.Controls.Add(botPanel);

            panel.Controls.Add(split);
            panel.Controls.Add(toolbar);

            // store for refresh
            _dgvInjected = dgvInjected;

            tabProcesses.Controls.Add(panel);
        }

        private DataGridView _dgvInjected;
        private List<ProcessRow> _allRows = new List<ProcessRow>();

        private class ProcessRow
        {
            public int Id;
            public string Name, Path;
        }

        private void RefreshProcessList()
        {
            _allRows.Clear();
            foreach (var p in Process.GetProcesses().OrderBy(x => x.ProcessName))
            {
                var path = "";
                try
                {
                    path = p.MainModule?.FileName ?? "";
                }
                catch
                {
                }

                _allRows.Add(new ProcessRow { Id = p.Id, Name = p.ProcessName, Path = path });
            }

            FilterProcessList();
            RefreshInjectedTab();
        }

        private void FilterProcessList()
        {
            var q = txtProcSearch.Text.Trim().ToLower();
            dgvProcesses.Rows.Clear();
            foreach (var r in _allRows)
            {
                if (q.Length > 0 && !r.Name.ToLower().Contains(q) && !r.Path.ToLower().Contains(q)) continue;
                dgvProcesses.Rows.Add(r.Id, r.Name, r.Path);
            }
        }

        private void RefreshInjectedTab()
        {
            if (_dgvInjected == null) return;
            _dgvInjected.Rows.Clear();
            foreach (var e in _injMgr.InjectedProcesses)
                _dgvInjected.Rows.Add(e.ProcessId, e.ProcessName, e.ProcessPath, e.IsInjected ? "Active" : "Pending");
        }

        private void OnInjectClick(object sender, EventArgs e)
        {
            var selected = GetSelectedProcessId(dgvProcesses);
            if (selected == null)
            {
                ShowInfo("Select a process first.");
                return;
            }

            try
            {
                var p = Process.GetProcessById(selected.Value);
                _injMgr.Inject(p);
                var dt = GetFakeTime();
                _injMgr.SetFakeTime(p.Id, dt.ToUniversalTime(), chkMockEnabled.Checked);
                RefreshInjectedTab();
                AppendLog($"Manually injected into [{p.Id}] {p.ProcessName}");
            }
            catch (Exception ex)
            {
                MessageBox.Show($"Injection failed:\n{ex.Message}", "Error",
                    MessageBoxButtons.OK, MessageBoxIcon.Error);
            }
        }

        private void OnEjectClick(object sender, EventArgs e)
        {
            var selected = GetSelectedProcessId(_dgvInjected);
            if (selected == null)
            {
                ShowInfo("Select an injected process first.");
                return;
            }

            _injMgr.Eject(selected.Value);
            RefreshInjectedTab();
        }

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
                BackColor = Color.FromArgb(30, 30, 35)
            };

            // Pattern input row
            var lblNew = new Label { Text = "Pattern:", AutoSize = true, Margin = new Padding(4, 12, 4, 0) };
            txtNewPattern = new TextBox
                { Width = 280, Margin = new Padding(0, 10, 4, 0), Text = "e.g. C:\\Games\\MyGame\\* or ^.*chrome.*$" };

            rdoGlob = new RadioButton
            {
                Text = "Glob", Checked = true, AutoSize = true, Margin = new Padding(4, 12, 4, 0),
                ForeColor = Color.FromArgb(220, 220, 220)
            };
            rdoRegex = new RadioButton
            {
                Text = "Regex", AutoSize = true, Margin = new Padding(4, 12, 4, 0),
                ForeColor = Color.FromArgb(220, 220, 220)
            };

            btnAddPattern = MakeButton("+ Add Rule", 100, Color.FromArgb(0, 150, 80));
            btnAddPattern.Margin = new Padding(8, 8, 4, 0);
            btnAddPattern.Click += OnAddPattern;

            btnRemovePattern = MakeButton("✕ Remove", 90, Color.FromArgb(180, 40, 40));
            btnRemovePattern.Margin = new Padding(4, 8, 4, 0);
            btnRemovePattern.Click += OnRemovePattern;

            chkWatcherEnabled = new CheckBox
            {
                Text = "Enable Auto-Inject Watcher",
                ForeColor = Color.LightGreen,
                AutoSize = true,
                Margin = new Padding(16, 12, 4, 0),
                Checked = true
            };
            chkWatcherEnabled.CheckedChanged += OnWatcherToggle;

            toolbar.Controls.AddRange(new Control[]
            {
                lblNew, txtNewPattern, rdoGlob, rdoRegex,
                btnAddPattern, btnRemovePattern, chkWatcherEnabled
            });

            var lblSection = MakeSectionLabel("Auto-Inject Rules (process path or name must match)");

            dgvPatterns = MakeGrid();
            dgvPatterns.Dock = DockStyle.Fill;
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

        private void OnWatcherToggle(object sender, EventArgs e)
        {
            if (chkWatcherEnabled.Checked)
            {
                _watcher.FakeUtc = GetFakeTime().ToUniversalTime();
                _watcher.MockEnabled = chkMockEnabled.Checked;
                _watcher.Start();
                AppendLog("Process watcher started.");
            }
            else
            {
                _watcher.Stop();
                AppendLog("Process watcher stopped.");
            }
        }

        // =====================================================================
        // Log Tab
        // =====================================================================
        private void BuildLogTab()
        {
            rtbLog = new RichTextBox
            {
                Dock = DockStyle.Fill,
                BackColor = Color.FromArgb(18, 18, 22),
                ForeColor = Color.FromArgb(180, 240, 180),
                Font = new Font("Consolas", 9f),
                ReadOnly = true,
                ScrollBars = RichTextBoxScrollBars.Vertical
            };

            btnClearLog = MakeButton("Clear", 70, Color.FromArgb(60, 60, 70));
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
            _injMgr.SetFakeTimeAll(dt, chkMockEnabled.Checked);
            _watcher.FakeUtc = dt;
            _watcher.MockEnabled = chkMockEnabled.Checked;

            if (chkAutoAdvance.Checked)
            {
                _fakeTimeBase = GetFakeTime();
                _advanceStartReal = DateTime.Now;
            }

            UpdateTimePreview();
        }

        private void UpdateTimePreview()
        {
            var dt = GetFakeTime();
            lblPreview.Text = chkMockEnabled.Checked
                ? $"Fake: {dt:yyyy-MM-dd HH:mm:ss} (local)"
                : "Pass-through (real time)";
        }

        private void ToggleAutoAdvance(object sender, EventArgs e)
        {
            if (chkAutoAdvance.Checked)
            {
                _fakeTimeBase = GetFakeTime();
                _advanceStartReal = DateTime.Now;
                _advanceTimer.Start();
            }
            else
            {
                _advanceTimer.Stop();
            }
        }

        private void AdvanceTick(object sender, EventArgs e)
        {
            if (!chkMockEnabled.Checked) return;
            var elapsed = DateTime.Now - _advanceStartReal;
            var advanced = _fakeTimeBase + elapsed;
            // Silently update dtpDate/dtpTime without triggering ValueChanged loop
            dtpDate.ValueChanged -= (s, ev) => UpdateTimePreview();
            dtpTime.ValueChanged -= (s, ev) => UpdateTimePreview();
            dtpDate.Value = advanced;
            dtpTime.Value = advanced;
            dtpDate.ValueChanged += (s, ev) => UpdateTimePreview();
            dtpTime.ValueChanged += (s, ev) => UpdateTimePreview();
            ApplyTime();
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

        private int? GetSelectedProcessId(DataGridView dgv)
        {
            if (dgv.SelectedRows.Count == 0) return null;
            var cell = dgv.SelectedRows[0].Cells[0].Value;
            return cell is int i ? i : (int?)null;
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
                BackgroundColor = Color.FromArgb(25, 25, 30),
                ForeColor = Color.FromArgb(220, 220, 220),
                GridColor = Color.FromArgb(50, 50, 55),
                BorderStyle = BorderStyle.None,
                RowHeadersVisible = false,
                AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.None,
                ColumnHeadersHeight = 28
            };
            g.EnableHeadersVisualStyles = false;
            g.ColumnHeadersDefaultCellStyle.BackColor = Color.FromArgb(40, 40, 48);
            g.ColumnHeadersDefaultCellStyle.ForeColor = Color.FromArgb(160, 200, 255);
            g.DefaultCellStyle.SelectionBackColor = Color.FromArgb(0, 90, 160);
            g.AlternatingRowsDefaultCellStyle.BackColor = Color.FromArgb(30, 30, 38);
            return g;
        }

        private static DataGridViewTextBoxColumn Col(string name, int w)
        {
            return new DataGridViewTextBoxColumn
                { HeaderText = name, Width = w, SortMode = DataGridViewColumnSortMode.NotSortable };
        }

        private static DataGridViewCheckBoxColumn BoolCol(string name)
        {
            return new DataGridViewCheckBoxColumn { HeaderText = name, Width = 65 };
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
                ForeColor = Color.FromArgb(130, 200, 255),
                Font = new Font("Segoe UI", 8.5f, FontStyle.Bold),
                Padding = new Padding(4, 2, 0, 0),
                BackColor = Color.FromArgb(35, 35, 42)
            };
        }

        private static void StyleTab(TabPage tab)
        {
            tab.BackColor = Color.FromArgb(30, 30, 35);
            tab.ForeColor = Color.FromArgb(220, 220, 220);
        }

        private void DrawTab(object sender, DrawItemEventArgs e)
        {
            var tab = (TabControl)sender;
            var page = tab.TabPages[e.Index];
            var rect = e.Bounds;
            var selected = e.Index == tab.SelectedIndex;

            using var bg = new SolidBrush(selected ? Color.FromArgb(0, 90, 160) : Color.FromArgb(40, 40, 48));
            e.Graphics.FillRectangle(bg, rect);

            var sf = new StringFormat
                { Alignment = StringAlignment.Center, LineAlignment = StringAlignment.Center };
            using var fg = new SolidBrush(selected ? Color.White : Color.FromArgb(180, 180, 180));
            e.Graphics.DrawString(page.Text, Font, fg, rect, sf);
        }

        private static void ShowInfo(string msg)
        {
            MessageBox.Show(msg, "Info", MessageBoxButtons.OK, MessageBoxIcon.Information);
        }

        protected override void OnFormClosing(FormClosingEventArgs e)
        {
            // Save config
            _config.AutoInjectEnabled = chkWatcherEnabled.Checked;
            _config.AutoAdvanceEnabled = chkAutoAdvance.Checked;
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