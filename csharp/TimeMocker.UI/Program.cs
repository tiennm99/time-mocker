using System;
using System.Windows.Forms;
using TimeMocker.UI.Forms;

namespace TimeMocker.UI
{
    internal static class Program
    {
        [STAThread]
        static void Main()
        {
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);

            // EasyHook requires elevated privileges for cross-process injection
            if (!IsElevated())
            {
                var result = MessageBox.Show(
                    "TimeMocker needs to run as Administrator to inject into other processes.\n\n" +
                    "Please restart as Administrator.",
                    "Elevation Required",
                    MessageBoxButtons.OKCancel,
                    MessageBoxIcon.Warning);

                if (result == DialogResult.OK)
                    RestartAsAdmin();
                return;
            }

            Application.Run(new MainForm());
        }

        private static bool IsElevated()
        {
            using var id = System.Security.Principal.WindowsIdentity.GetCurrent();
            var principal = new System.Security.Principal.WindowsPrincipal(id);
            return principal.IsInRole(System.Security.Principal.WindowsBuiltInRole.Administrator);
        }

        private static void RestartAsAdmin()
        {
            var info = new System.Diagnostics.ProcessStartInfo
            {
                FileName        = Application.ExecutablePath,
                UseShellExecute = true,
                Verb            = "runas"
            };
            try { System.Diagnostics.Process.Start(info); }
            catch { /* user cancelled UAC */ }
        }
    }
}
