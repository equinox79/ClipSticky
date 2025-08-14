using System;
using System.Windows;
using System.Windows.Forms;
using Application = System.Windows.Application;
using MessageBox = System.Windows.MessageBox;

namespace ClipSticky
{
    public partial class App : Application
    {
        private NotifyIcon? _tray;
        private MainWindow? _window;
        private bool _monitoring = true;

        protected override void OnStartup(StartupEventArgs e)
        {
            base.OnStartup(e);

            _window = new MainWindow();
            _window.Hide(); // 起動時は表示しない（トレイ常駐）

            // タスクトレイ
            _tray = new NotifyIcon
            {
                Icon = System.Drawing.SystemIcons.Information,
                Visible = true,
                Text = "ClipSticky - 最新のクリップボードを表示"
            };

            var menu = new ContextMenuStrip();
            var showItem = new ToolStripMenuItem("付箋を表示/非表示", null, (_, __) => ToggleWindow());
            var pauseItem = new ToolStripMenuItem("監視 一時停止/再開", null, (_, __) => ToggleMonitoring());
            var exitItem = new ToolStripMenuItem("終了", null, (_, __) => ExitApp());

            menu.Items.Add(showItem);
            menu.Items.Add(pauseItem);
            menu.Items.Add(new ToolStripSeparator());
            menu.Items.Add(exitItem);

            _tray.ContextMenuStrip = menu;
            _tray.DoubleClick += (_, __) => ToggleWindow();
        }

        private void ToggleWindow()
        {
            if (_window == null) return;
            if (_window.IsVisible)
            {
                _window.Hide();
            }
            else
            {
                _window.Show();
                _window.Activate();
            }
        }

        private void ToggleMonitoring()
        {
            _monitoring = !_monitoring;
            if (_window != null) _window.IsMonitoring = _monitoring;
            _tray!.BalloonTipTitle = "ClipSticky";
            _tray.BalloonTipText = _monitoring ? "監視を再開しました" : "監視を一時停止しました";
            _tray.ShowBalloonTip(1000);
        }

        private void ExitApp()
        {
            try
            {
                _tray!.Visible = false;
                _tray!.Dispose();
            }
            catch { /* ignore */ }

            _window?.Close();
            Shutdown();
        }
    }
}
