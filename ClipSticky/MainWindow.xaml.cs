using System;
using System.Configuration; // 追加
using System.Runtime.InteropServices;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Interop;
using System.Windows.Media;
using System.Windows.Media.Imaging;


namespace ClipSticky
{
    public partial class MainWindow : Window
    {
        // Win32: クリップボード更新メッセージ
        private const int WM_CLIPBOARDUPDATE = 0x031D;

        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool AddClipboardFormatListener(IntPtr hwnd);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool RemoveClipboardFormatListener(IntPtr hwnd);

        public string TitleText { get; set; } = "最新のクリップボード";
        public string StatusText
        {
            get => _statusText;
            set { _statusText = value; DataContext = null; DataContext = this; }
        }
        private string _statusText = "監視中";

        public UIElement? ContentView
        {
            get => _contentView;
            set { _contentView = value; DataContext = null; DataContext = this; }
        }
        private UIElement? _contentView;

        private HwndSource? _source;
        private bool _isMonitoring = true;
        public bool IsMonitoring
        {
            get => _isMonitoring;
            set
            {
                _isMonitoring = value;
                StatusText = value ? "監視中" : "一時停止中";
            }
        }

        public MainWindow()
        {
            InitializeComponent();
            DataContext = this;

            // 前回のウィンドウ位置を復元
            if (Settings.Default.WindowLeft >= 0 && Settings.Default.WindowTop >= 0)
            {
                this.Left = Settings.Default.WindowLeft;
                this.Top = Settings.Default.WindowTop;
            }

            // 初期は最新クリップボードを試しに表示
            TryRenderClipboard();

            // 起動時にフォームを表示・前面化
            this.Show();
            //this.Activate();

            // マウスダウンでウィンドウ移動
            this.MouseLeftButtonDown += (s, e) =>
            {
                if (e.ButtonState == System.Windows.Input.MouseButtonState.Pressed)
                    this.DragMove();
            };

            // ウィンドウ位置変更時に保存
            this.LocationChanged += (s, e) =>
            {
                Settings.Default.WindowLeft = this.Left;
                Settings.Default.WindowTop = this.Top;
                Settings.Default.Save();
            };
        }

        protected override void OnSourceInitialized(EventArgs e)
        {
            base.OnSourceInitialized(e);
            _source = HwndSource.FromHwnd(new WindowInteropHelper(this).Handle);
            _source.AddHook(WndProc);

            // クリップボード更新イベントを購読
            AddClipboardFormatListener(_source.Handle);
        }

        protected override void OnClosed(EventArgs e)
        {
            // ウィンドウ位置を保存
            Settings.Default.WindowLeft = this.Left;
            Settings.Default.WindowTop = this.Top;
            Settings.Default.Save();

            if (_source != null)
            {
                RemoveClipboardFormatListener(_source.Handle);
                _source.RemoveHook(WndProc);
            }
            base.OnClosed(e);
        }

        private IntPtr WndProc(IntPtr hwnd, int msg, IntPtr wParam, IntPtr lParam, ref bool handled)
        {
            if (msg == WM_CLIPBOARDUPDATE)
            {
                if (IsMonitoring)
                {
                    TryRenderClipboard();
                    // クリップボード更新時にウィンドウを前面に表示
                    this.Show();
                    //this.Activate();
                    this.Topmost = true;
                    this.Topmost = false; // Topmost解除（必要なら）
                }
                handled = false;
            }
            return IntPtr.Zero;
        }

        private void TryRenderClipboard()
        {
            try
            {
                // クリップボードが他のプロセスでロックされている場合、リトライする
                int retry = 5;
                while (retry-- > 0)
                {
                    try
                    {
                        if (System.Windows.Clipboard.ContainsImage())
                        {
                            var img = System.Windows.Clipboard.GetImage();
                            if (img != null)
                            {
                                double maxW = 340, maxH = 140;
                                var image = new System.Windows.Controls.Image
                                {
                                    Source = BitmapFrame.Create(img),
                                    Stretch = Stretch.Uniform,
                                    MaxWidth = maxW,
                                    MaxHeight = maxH,
                                    SnapsToDevicePixels = true
                                };
                                ContentView = image;
                                TitleText = "最新のクリップボード（画像）";
                                StatusText = IsMonitoring ? "監視中（画像を検出）" : "一時停止中";
                                return;
                            }
                        }

                        if (System.Windows.Clipboard.ContainsText())
                        {
                            var text = System.Windows.Clipboard.GetText();
                            var tb = new TextBlock
                            {
                                Text = text,
                                TextWrapping = TextWrapping.Wrap,
                                FontFamily = new System.Windows.Media.FontFamily("Yu Gothic UI"),
                                MaxWidth = 340
                            };
                            ContentView = tb;
                            TitleText = "最新のクリップボード（テキスト）";
                            StatusText = IsMonitoring ? "監視中（テキストを検出）" : "一時停止中";
                            return;
                        }

                        if (System.Windows.Clipboard.ContainsFileDropList())
                        {
                            var files = System.Windows.Clipboard.GetFileDropList();
                            var sb = string.Join(Environment.NewLine, files);
                            ContentView = new TextBlock { Text = sb, TextWrapping = TextWrapping.Wrap, MaxWidth = 340 };
                            TitleText = "最新のクリップボード（ファイル）";
                            StatusText = IsMonitoring ? "監視中（ファイルを検出）" : "一時停止中";
                            return;
                        }

                        ContentView = new TextBlock
                        {
                            Text = "未知の形式（または空）です。",
                            Foreground = System.Windows.Media.Brushes.Gray
                        };
                        TitleText = "最新のクリップボード";
                        StatusText = IsMonitoring ? "監視中（未知の形式）" : "一時停止中";
                        return;
                    }
                    catch (System.Runtime.InteropServices.COMException ex) when ((uint)ex.ErrorCode == 0x800401D0)
                    {
                        // クリップボードがロックされている場合は少し待ってリトライ
                        System.Threading.Thread.Sleep(50);
                        continue;
                    }
                }

                // リトライしてもダメならエラー表示
                ContentView = new TextBlock
                {
                    Text = "取得エラー: クリップボードが他のプロセスで使用中です。",
                    TextWrapping = TextWrapping.Wrap
                };
                TitleText = "最新のクリップボード（エラー）";
                StatusText = IsMonitoring ? "監視中（エラー）" : "一時停止中";
            }
            catch (Exception ex)
            {
                ContentView = new TextBlock
                {
                    Text = "取得エラー: " + ex.Message,
                    TextWrapping = TextWrapping.Wrap
                };
                TitleText = "最新のクリップボード（エラー）";
                StatusText = IsMonitoring ? "監視中（エラー）" : "一時停止中";
            }
        }

        private void CloseButton_Click(object sender, RoutedEventArgs e)
        {
            this.Hide();
        }
    }
}
