param([switch]$Cold, [string]$ExistingModel = 'GLM-5.2')
$ErrorActionPreference = 'Stop'
$repo = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$testBinary = Get-ChildItem -LiteralPath (Join-Path $repo 'src-tauri/target/debug/deps') -Filter 'at_switch_lib-*.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $testBinary) { throw 'Run cargo test --lib --no-run first.' }
$targetPath = [System.IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Programs/Trae CN/Trae CN.exe'))
if (-not (Test-Path -LiteralPath $targetPath)) { throw 'New TRAE is not installed at the standard path.' }
if ((Get-Item -LiteralPath $targetPath).VersionInfo.ProductVersion -ne '3.4.2') { throw 'This test is gated to TRAE 3.4.2.' }
$otherNames = @('WorkBuddy.exe','CodeBuddy CN.exe','QClaw.exe','AutoClaw.exe','Codex.exe','DuMate.exe','ima.copilot.exe','TRAE SOLO CN.exe')
function Get-AgentProcesses {
    @(Get-CimInstance Win32_Process | Where-Object { $_.Name -in ($otherNames + @('Trae CN.exe')) -and $_.CommandLine -notmatch '--type=|\.cjs' })
}
$before = Get-AgentProcesses
$otherBefore = @($before | Where-Object Name -in $otherNames | ForEach-Object { "$($_.Name):$($_.ProcessId)" } | Sort-Object)
$previous = @($before | Where-Object ExecutablePath -eq $targetPath | ForEach-Object { [int]$_.ProcessId })
if ($previous.Count -ne 1) { throw 'Start exactly one TRAE main instance before acceptance.' }
$resultDir = Join-Path $env:TEMP ('at-switch-trae-acceptance-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $resultDir | Out-Null
$stdout = Join-Path $resultDir 'stdout.log'
$stderr = Join-Path $resultDir 'stderr.log'
$env:AT_SWITCH_TRAE_LIVE_MUTATION = '1'
$env:AT_SWITCH_TRAE_KIND = 'trae'
$env:AT_SWITCH_TRAE_LIVE_COLD = $(if ($Cold) { '1' } else { '0' })
$env:AT_SWITCH_TRAE_LIVE_SEQUENCE = $(if ($Cold) { '0' } else { '1' })
$env:AT_SWITCH_TRAE_LIVE_EXISTING_MODEL = $ExistingModel
$test = Start-Process -FilePath $testBinary.FullName -ArgumentList @('agents::trae::tests::live_native_bridge_applies_and_restores_temporary_model','--ignored','--exact','--nocapture') -WorkingDirectory $repo -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
$test.Handle | Out-Null
$events = [System.Collections.Generic.List[object]]::new()
$phase = 'setup'
do {
    if (Test-Path -LiteralPath $stderr) {
        $markers = @(Get-Content -LiteralPath $stderr | Where-Object { $_ -match '^TRAE_LIVE_PHASE ' })
        if ($markers.Count) { $phase = $markers[-1].Substring(16) }
    }
    $now = Get-AgentProcesses
    $current = @($now | Where-Object ExecutablePath -eq $targetPath | ForEach-Object { [int]$_.ProcessId })
    foreach ($id in $current) { if ($id -notin $previous) { $events.Add([pscustomobject]@{ action='start'; phase=$phase; pid=$id }) } }
    foreach ($id in $previous) { if ($id -notin $current) { $events.Add([pscustomobject]@{ action='exit'; phase=$phase; pid=$id }) } }
    $previous = $current
    Start-Sleep -Milliseconds 500
    $test.Refresh()
} while (-not $test.HasExited)
$test.WaitForExit()
Get-Content -LiteralPath $stdout
Get-Content -LiteralPath $stderr
$otherAfter = @(Get-AgentProcesses | Where-Object Name -in $otherNames | ForEach-Object { "$($_.Name):$($_.ProcessId)" } | Sort-Object)
$unchanged = -not (Compare-Object $otherBefore $otherAfter)
$expectedStarts = $(if ($Cold) { 3 } else { 1 })
$starts = @($events | Where-Object action -eq 'start').Count
$exits = @($events | Where-Object action -eq 'exit').Count
$passed = $test.ExitCode -eq 0 -and $unchanged -and $starts -eq $expectedStarts -and $exits -eq $expectedStarts
$result = [pscustomobject]@{ mode=$(if($Cold){'cold'}else{'running'}); passed=$passed; exitCode=$test.ExitCode; starts=$starts; exits=$exits; otherAgentProcessesUnchanged=$unchanged; events=$events.ToArray(); logs=$resultDir }
$result | ConvertTo-Json -Depth 5
if (-not $passed) { throw 'TRAE Windows acceptance failed; inspect the retained logs.' }
