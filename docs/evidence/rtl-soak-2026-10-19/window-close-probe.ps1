param([string]$Executable = '.\target\release\sdr-app.exe')

$env:WGPU_BACKEND = 'dx12'
$env:RUSTSDR_CONFIG_DIR = Join-Path (Get-Location) 'target/window-close-probe-config'
New-Item -ItemType Directory -Force -Path $env:RUSTSDR_CONFIG_DIR | Out-Null

$source = @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class GuiWindowClose
{
    private delegate bool EnumWindowsCallback(IntPtr window, IntPtr unused);
    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsCallback callback, IntPtr unused);
    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowTextW(IntPtr window, StringBuilder text, int capacity);
    [DllImport("user32.dll")]
    private static extern bool PostMessageW(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    public static IntPtr Find(int targetProcess)
    {
        IntPtr result = IntPtr.Zero;
        EnumWindows((window, unused) => {
            uint processId;
            GetWindowThreadProcessId(window, out processId);
            if (processId != (uint)targetProcess) return true;
            var title = new StringBuilder(256);
            GetWindowTextW(window, title, title.Capacity);
            if (!title.ToString().Contains("RustSDR")) return true;
            result = window;
            return false;
        }, IntPtr.Zero);
        return result;
    }

    public static bool Close(IntPtr window)
    {
        return PostMessageW(window, 0x0010, IntPtr.Zero, IntPtr.Zero);
    }
}
'@

Add-Type -TypeDefinition $source
$process = Start-Process -FilePath $Executable -ArgumentList @('--gui') -PassThru -WindowStyle Hidden
try {
    $window = [IntPtr]::Zero
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        $window = [GuiWindowClose]::Find($process.Id)
        if ($window -ne [IntPtr]::Zero) { break }
        Start-Sleep -Milliseconds 250
    }
    if ($window -eq [IntPtr]::Zero) { throw "RustSDR window was not created" }
    Start-Sleep -Seconds 5
    if (-not [GuiWindowClose]::Close($window)) { throw "WM_CLOSE could not be posted" }
    if (-not $process.WaitForExit(15000)) { throw "Application did not exit within 15 seconds of WM_CLOSE" }
    Write-Output "pid=$($process.Id) window=$window exit=$($process.ExitCode)"
    if ($process.ExitCode -ne 0) { throw "Application exited with an error" }
}
finally {
    if (-not $process.HasExited) {
        $process.Kill()
        $process.WaitForExit()
    }
    $process.Dispose()
}
