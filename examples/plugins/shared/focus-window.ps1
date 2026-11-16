param([Parameter(Mandatory=$true)][int]$ProcessId)
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class WindowFocus {
  [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr hWnd, int nCmdShow);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
}
'@
$process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
if ($null -eq $process) { exit 1 }
$process.Refresh()
if ($process.MainWindowHandle -eq [IntPtr]::Zero) { exit 1 }
[WindowFocus]::ShowWindowAsync($process.MainWindowHandle, 9) | Out-Null
if (-not [WindowFocus]::SetForegroundWindow($process.MainWindowHandle)) { exit 1 }
