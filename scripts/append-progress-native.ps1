param([Parameter(Mandatory=$true)][string]$Config)
# Normal owner console only. Build/staging are separate; preserve every failed attempt.
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
function Hash([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Require($Condition,[string]$Message) { if(-not $Condition) { throw $Message } }
function ReadJson([string]$Path) { Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json }
function Context {
    $id=[Security.Principal.WindowsIdentity]::GetCurrent($false)
    Require ($null -eq [Security.Principal.WindowsIdentity]::GetCurrent($true)) 'impersonating token'
    Require ($id.User.Value -ceq 'S-1-5-21-2025389028-1701693243-395167910-1000' -and $id.Owner.Value -ceq $id.User.Value) 'normal owner token required'
    Require (-not [Security.Principal.WindowsPrincipal]::new($id).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'nonadmin console required'
}
Context
$cfg=ReadJson $Config
Require ($cfg.schema -ceq 'append-progress-gate-v1' -and $cfg.host -ceq 'windows') 'gate schema/host'
$cases=@(ReadJson ($cfg.source+'\scripts\append-progress-cases.json'))
Require ($cases.Count -eq 55 -and @($cases.name|Sort-Object -Unique).Count -eq 55) 'case inventory'
$expected=@($cases|Where-Object {$_.windows -gt 0 -or $_.pure -or $_.child_entry})
$selected=@($expected|Where-Object {-not $_.child_entry}|Sort-Object @{Expression={-not $_.pure}})
Require ($selected.Count -eq 52 -and ($selected|Measure-Object -Property windows -Sum).Sum -eq 174) 'finite gate scope'
$manifest=ReadJson $cfg.fixture_manifest
Require ($manifest.schema -ceq 'append-progress-fixtures-v1' -and $manifest.host -ceq 'windows' -and $manifest.run -cmatch '^append-gate-[a-z0-9-]+$') 'fixture schema/host/run'
Require ($manifest.parent -cmatch '^C:\\append-gate-[a-z0-9-]+$') 'fresh parent namespace'
$wanted=@($selected|ForEach-Object {
    $case=$_
    for($n=1;$n -le $case.windows;$n++) { $case.name+'|'+$n+'|'+$manifest.parent+'\'+$manifest.run+'-'+$case.id+'-'+$n.ToString('00') }
})
$actual=@($manifest.allocations|ForEach-Object {
    Require ($_.version -eq 1 -and $_.run -ceq $manifest.run) 'assignment identity'
    Require (-not (Test-Path -LiteralPath $_.root)) 'fixture collision; preserve'
    $_.case+'|'+$_.ordinal+'|'+$_.root
})
Require ($actual.Count -eq $wanted.Count -and @(Compare-Object $wanted $actual -CaseSensitive).Count -eq 0) 'complete exact fixture assignments'
$sourceManifest=ReadJson $cfg.source_manifest
function Verify {
    foreach($pin in $cfg.pins.PSObject.Properties) { Require ((Hash $pin.Name) -ceq $pin.Value) ('identity drift '+$pin.Name) }
    foreach($f in $sourceManifest.files) { Require ((Hash ($cfg.source+'\'+$f.path)) -ceq $f.sha256) ('source drift '+$f.path) }
}
foreach($key in @('nextest','cargo_metadata','binaries_metadata','nextest_config','fixture_manifest','source_manifest')) {
    Require ($null -ne $cfg.pins.PSObject.Properties[$cfg.$key]) ('unbound execution input '+$key)
}
Verify
Require (-not (Test-Path -LiteralPath $cfg.evidence)) 'prior attempt preserved; no automatic rerun'
$null=New-Item -ItemType Directory -Path $cfg.evidence
Get-ChildItem Env: | Where-Object {$_.Name -match '^(POINTBREAK|CARGO|RUST|NEXTEST)'} | ForEach-Object { Remove-Item ('Env:'+ $_.Name) }
foreach($entry in $cfg.environment.PSObject.Properties) { [Environment]::SetEnvironmentVariable($entry.Name,[string]$entry.Value,'Process') }
$env:POINTBREAK_APPEND_FIXTURES=$cfg.fixture_manifest
$env:POINTBREAK_APPEND_FIXTURES_SHA256=Hash $cfg.fixture_manifest
foreach($pair in @(@('POINTBREAK_ENROLLMENT_ACL_SCRIPT','POINTBREAK_ENROLLMENT_ACL_SHA256'),@('POINTBREAK_ENROLLMENT_ACL_BINDING','POINTBREAK_ENROLLMENT_ACL_BINDING_SHA256'))) {
    $path=[Environment]::GetEnvironmentVariable($pair[0]);$hash=[Environment]::GetEnvironmentVariable($pair[1])
    Require ($cfg.pins.$path -ceq $hash -and (Hash $path) -ceq $hash) 'unbound ACL input'
}
Require ((Hash 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe') -ceq $env:POINTBREAK_APPEND_POWERSHELL_SHA256) 'original PowerShell identity'
Set-Location -LiteralPath $cfg.source
$common=@('--cargo-metadata',$cfg.cargo_metadata,'--binaries-metadata',$cfg.binaries_metadata,'--config-file',$cfg.nextest_config,'--user-config-file','none')
function InvokeCase([string]$Label,[string[]]$Arguments) {
    $folder=$cfg.evidence+'\'+$Label
    $null=New-Item -ItemType Directory -Path $folder
    $started=[DateTime]::UtcNow
    $prior=$ErrorActionPreference
    $global:LASTEXITCODE=$null
    try { $ErrorActionPreference='Continue'; & $cfg.nextest nextest @Arguments 1> ($folder+'\stdout') 2> ($folder+'\stderr');$code=$LASTEXITCODE }
    finally { $ErrorActionPreference=$prior }
    $ended=[DateTime]::UtcNow
    $record=@{arguments=$Arguments;exit_code=$code;started=$started.ToString('o');ended=$ended.ToString('o')}
    [IO.File]::WriteAllText(($folder+'\capture.json'),($record|ConvertTo-Json -Depth 5))
    return @{folder=$folder;code=$code;started=$started;ended=$ended}
}
$r=InvokeCase 'discovery' (@('list')+$common+@('--message-format','json','--run-ignored','all','-E','test(/^session::append_progress::tests::/)'))
Require ($null -ne $r.code -and $r.code -eq 0) 'discovery failed; preserve'
$listing=ReadJson ($r.folder+'\stdout')
$actual=@($listing.'rust-suites'.PSObject.Properties|ForEach-Object {$_.Value.testcases.PSObject.Properties}|Where-Object {$_.Value.'filter-match'.status -eq 'matches'}|ForEach-Object {$_.Name+'|'+$_.Value.ignored})
$wanted=@($expected|ForEach-Object {$_.name+'|'+($_.windows_gate -or $_.child_entry)})
Require ($actual.Count -eq $wanted.Count -and @(Compare-Object $wanted $actual -CaseSensitive).Count -eq 0) 'compiled case/ignore inventory mismatch'
$runIds=@();$passed=@()
foreach($case in $selected) {
    Context
    Require (-not (Test-Path -LiteralPath $cfg.junit)) 'uncollected JUnit; preserve'
    $r=InvokeCase $case.id (@('run')+$common+@('--run-ignored','all','--no-tests','fail','--test-threads','1','--retries','0','--failure-output','immediate-final','--success-output','final','-E',('test(='+$case.name+')')))
    $modified=$null;$report=$r.folder+'\junit.xml'
    if(Test-Path -LiteralPath $cfg.junit) { $modified=(Get-Item -LiteralPath $cfg.junit).LastWriteTimeUtc;[IO.File]::Move($cfg.junit,$report) }
    Require ($null -ne $r.code -and $r.code -eq 0 -and $null -ne $modified) ('failed '+$case.id+'; preserve, no replay')
    [xml]$xml=[IO.File]::ReadAllText($report)
    $uuid=[Guid]::Parse($xml.testsuites.uuid).ToString()
    Require ($modified -ge $r.started -and $modified -le $r.ended -and $runIds -notcontains $uuid) 'stale/duplicate result'
    $actual=@($xml.SelectNodes('//testcase')|ForEach-Object {$_.name})
    Require ($actual.Count -eq 1 -and $actual[0] -ceq $case.name -and @($xml.SelectNodes('//failure|//error|//skipped')).Count -eq 0) 'exact passing result membership'
    $runIds+=@($uuid);$passed+=@(@{name=$case.name;junit_sha256=(Hash $report);run_uuid=$uuid})
    Write-Output ('PASS '+$case.id)
}
Verify
[IO.File]::WriteAllText(($cfg.evidence+'\passed.json'),(@{config_sha256=(Hash $Config);cases=$passed;diagnostic_only=$true}|ConvertTo-Json -Depth 6))
