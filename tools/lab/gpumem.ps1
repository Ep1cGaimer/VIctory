param([int]$Seconds=80)
$end=(Get-Date).AddSeconds($Seconds)
while((Get-Date) -lt $end){
  try {
    $p = Get-Process -Name 'victory-*' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($p) {
      $ded = (Get-Counter "\GPU Process Memory(pid_$($p.Id)*)\Dedicated Usage" -ErrorAction SilentlyContinue).CounterSamples | Measure-Object CookedValue -Sum
      $sh = (Get-Counter "\GPU Process Memory(pid_$($p.Id)*)\Shared Usage" -ErrorAction SilentlyContinue).CounterSamples | Measure-Object CookedValue -Sum
      $os = Get-CimInstance Win32_OperatingSystem
      $p.Refresh()
      '{0:HH:mm:ss} dedicated {1:N0} MB shared {2:N0} MB private {3:N0} MB commitfree {4:N0} MB' -f (Get-Date), ($ded.Sum/1MB), ($sh.Sum/1MB), ($p.PrivateMemorySize64/1MB), ($os.FreeVirtualMemory/1KB)
    }
  } catch {}
  Start-Sleep -Milliseconds 1500
}
