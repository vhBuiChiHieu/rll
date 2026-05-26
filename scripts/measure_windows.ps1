param(
    [string]$Binary = "target/release/rll.exe",
    [int]$Entries = 10000,
    [int]$Runs = 5
)

$ErrorActionPreference = "Stop"
$BinaryPath = (Resolve-Path $Binary).Path
$BenchDir = Join-Path ([System.IO.Path]::GetTempPath()) ("rll-perf-{0}-{1}" -f $PID, [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
New-Item -ItemType Directory -Path $BenchDir | Out-Null

try {
    for ($i = 0; $i -lt $Entries; $i++) {
        Set-Content -Path (Join-Path $BenchDir ("file-{0:D6}.txt" -f $i)) -Value "0123456789abcdef" -NoNewline
    }

    $totalMs = 0.0
    $peakWorkingSet = 0L

    for ($run = 0; $run -lt $Runs; $run++) {
        $process = [System.Diagnostics.Process]::new()
        $process.StartInfo.FileName = $BinaryPath
        $process.StartInfo.WorkingDirectory = $BenchDir
        $process.StartInfo.UseShellExecute = $false
        $process.StartInfo.RedirectStandardOutput = $true
        $process.StartInfo.RedirectStandardError = $true
        $process.StartInfo.CreateNoWindow = $true

        $watch = [System.Diagnostics.Stopwatch]::StartNew()
        [void]$process.Start()

        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()

        while (-not $process.HasExited) {
            $process.Refresh()
            if ($process.PeakWorkingSet64 -gt $peakWorkingSet) {
                $peakWorkingSet = $process.PeakWorkingSet64
            }
            Start-Sleep -Milliseconds 1
        }

        $process.WaitForExit()
        $process.Refresh()
        if ($process.PeakWorkingSet64 -gt $peakWorkingSet) {
            $peakWorkingSet = $process.PeakWorkingSet64
        }
        $watch.Stop()

        $stderrText = $stderrTask.Result
        [void]$stdoutTask.Result

        if ($process.ExitCode -ne 0) {
            Write-Error $stderrText
            exit 1
        }

        $totalMs += $watch.Elapsed.TotalMilliseconds
        $process.Dispose()
    }

    $binarySize = (Get-Item $BinaryPath).Length
    $avgMs = $totalMs / $Runs
    $avgPerEntryNs = [math]::Round(($avgMs * 1000000.0) / [math]::Max($Entries, 1))

    "entries: $Entries"
    "runs: $Runs"
    "avg_wall_time_ms: {0:N3}" -f $avgMs
    "avg_per_entry_ns: $avgPerEntryNs"
    "binary_size_bytes: $binarySize"
    "peak_working_set_bytes: $peakWorkingSet"
}
finally {
    Remove-Item -Recurse -Force $BenchDir -ErrorAction SilentlyContinue
}
