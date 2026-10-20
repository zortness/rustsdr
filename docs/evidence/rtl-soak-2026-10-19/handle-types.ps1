param([int]$TargetPid, [string]$OutputPath)

$source = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Threading;

public static class HandleTypeSnapshot
{
    [DllImport("ntdll.dll")]
    private static extern int NtQuerySystemInformation(int infoClass, IntPtr buffer, int length, out int returnLength);

    public static string Snapshot(int targetPid)
    {
        using (var eventHandle = new EventWaitHandle(false, EventResetMode.AutoReset))
        {
            ulong ownEvent = (ulong)eventHandle.SafeWaitHandle.DangerousGetHandle().ToInt64();
            int ownPid = System.Diagnostics.Process.GetCurrentProcess().Id;
            int capacity = 8 * 1024 * 1024;
            for (int attempt = 0; attempt < 7; attempt++)
            {
                IntPtr buffer = Marshal.AllocHGlobal(capacity);
                try
                {
                    int needed;
                    int status = NtQuerySystemInformation(64, buffer, capacity, out needed);
                    if (status == unchecked((int)0xC0000004))
                    {
                        capacity = Math.Max(capacity * 2, needed + 4096);
                        continue;
                    }
                    if (status != 0)
                        throw new Exception("NtQuerySystemInformation returned 0x" + status.ToString("X8"));

                    long count = Marshal.ReadInt64(buffer);
                    var types = new SortedDictionary<int, int>();
                    int eventType = -1;
                    for (long i = 0; i < count; i++)
                    {
                        IntPtr entry = IntPtr.Add(buffer, checked((int)(16 + i * 40)));
                        long pid = Marshal.ReadInt64(entry, 8);
                        long value = Marshal.ReadInt64(entry, 16);
                        int type = (ushort)Marshal.ReadInt16(entry, 30);
                        if (pid == ownPid && (ulong)value == ownEvent)
                            eventType = type;
                        if (pid == targetPid)
                        {
                            int current;
                            types.TryGetValue(type, out current);
                            types[type] = current + 1;
                        }
                    }
                    var lines = new List<string>();
                    lines.Add("event_type_index=" + eventType + ",target_pid=" + targetPid);
                    foreach (var pair in types)
                        lines.Add(pair.Key + "," + pair.Value);
                    return string.Join(Environment.NewLine, lines);
                }
                finally
                {
                    Marshal.FreeHGlobal(buffer);
                }
            }
            throw new Exception("System handle table kept growing during snapshot");
        }
    }
}
'@

Add-Type -TypeDefinition $source
$snapshot = [HandleTypeSnapshot]::Snapshot($TargetPid)
if ($OutputPath) {
    Set-Content -LiteralPath $OutputPath -Value $snapshot
}
$snapshot
