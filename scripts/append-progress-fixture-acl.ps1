param([ValidateSet('Prepare','Negative','Inspect')][string]$Mode,[string]$Root,[string]$Target)
$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
if((Get-FileHash -LiteralPath $env:POINTBREAK_ENROLLMENT_ACL_BINDING -Algorithm SHA256).Hash.ToLower() -ne $env:POINTBREAK_ENROLLMENT_ACL_BINDING_SHA256){throw 'binding identity'}
$binding = Get-Content -LiteralPath $env:POINTBREAK_ENROLLMENT_ACL_BINDING -Raw | ConvertFrom-Json
if((Get-FileHash -LiteralPath $env:POINTBREAK_APPEND_FIXTURES -Algorithm SHA256).Hash.ToLower() -ne $env:POINTBREAK_APPEND_FIXTURES_SHA256){throw 'manifest identity'}
$manifest=Get-Content -LiteralPath $env:POINTBREAK_APPEND_FIXTURES -Raw | ConvertFrom-Json
if($manifest.schema -cne 'append-progress-fixtures-v1' -or $manifest.host -cne 'windows' -or $manifest.run -notmatch '^append-gate-[a-z0-9-]+$'){throw 'manifest schema/host/run'}
$parent=$manifest.parent
if($parent -notmatch '^C:\\append-gate-[a-z0-9-]+$' -or $binding.fixture_parent -cne $parent){throw 'fresh parent namespace'}
$fsutil='C:\Windows\System32\fsutil.exe'
if((Get-FileHash -LiteralPath $fsutil -Algorithm SHA256).Hash.ToLower() -cne $binding.fsutil_sha256){throw 'fsutil identity'}
function Token {
    $id=[Security.Principal.WindowsIdentity]::GetCurrent($false)
    if($null -ne [Security.Principal.WindowsIdentity]::GetCurrent($true) -or $id.User.Value -ne $id.Owner.Value -or $id.User.Value -ne 'S-1-5-21-2025389028-1701693243-395167910-1000'){throw 'token/default-owner/impersonation'}
    return $id.User
}
function FileId([string]$p) {
    $value=(& $fsutil file queryfileid $p) -join ''
    if($LASTEXITCODE -ne 0){throw 'identity query'}
    return $value
}
function ReadDescriptor([string]$p,[bool]$rootKind) {
    $item=Get-Item -LiteralPath $p -Force
    if($item.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'reparse object'}
    $before=FileId $p
    $acl=Get-Acl -LiteralPath $p
    $raw=[Security.AccessControl.RawSecurityDescriptor]::new($acl.Sddl)
    $rows=@(); $seen=@{}
    $valid=($raw.Owner.Value -eq $u.Value -and $null -ne $raw.DiscretionaryAcl -and $raw.DiscretionaryAcl.Revision -eq 2)
    foreach($ace in $raw.DiscretionaryAcl){
        $sid=$ace.SecurityIdentifier.Value; $flags=[int]$ace.AceFlags
        $flagsOk=if($rootKind){$flags -eq 3}else{if($item.PSIsContainer){$flags -eq 3 -or $flags -eq 19}else{$flags -eq 0 -or $flags -eq 16}}
        $valid=$valid -and ([int]$ace.AceType -eq 0) -and ($ace.AccessMask -eq 2032127) -and $flagsOk -and ($sid -eq $u.Value -or $sid -eq 'S-1-5-18') -and (-not $seen.ContainsKey($sid))
        $seen[$sid]=$true
        $rows+=@{type=[int]$ace.AceType; flags=$flags; mask=$ace.AccessMask; sid=$sid; binary_length=$ace.BinaryLength}
    }
    $valid=$valid -and ($rows.Count -ge 1 -and $rows.Count -le 2) -and $seen.ContainsKey($u.Value) -and ((-not $rootKind) -or $acl.AreAccessRulesProtected)
    $after=FileId $p
    if($before -ne $after){throw 'identity moved'}
    return @{path=$p;owner=$raw.Owner.Value;protected=$acl.AreAccessRulesProtected;acl_revision=$raw.DiscretionaryAcl.Revision;aces=$rows;sddl=$acl.Sddl;identity_before=$before;identity_after=$after;compliant=[bool]$valid}
}
$u=Token
$assignments=@($manifest.allocations | Where-Object {$_.root -ceq $Root})
if($assignments.Count -ne 1){throw 'ambiguous/unassigned root'}
$assignment=$assignments[0]
if($assignment.version -ne 1 -or $assignment.run -cne $manifest.run -or [IO.Path]::GetDirectoryName($Root) -cne $parent -or [IO.Path]::GetFileName($Root) -notmatch '^append-gate-[a-z0-9-]+-(S|R)[0-9]{2}-[0-9]{2}$'){throw 'unassigned outer-root spelling'}
if($Target -ne $Root -and (-not $Target.StartsWith($Root+'\',[StringComparison]::Ordinal))){throw 'oracle target escaped root'}
if($Target.Split('\') -contains '..'){throw 'oracle traversal'}
if($Mode -eq 'Negative' -and ($assignment.case -cne 'session::append_progress::tests::enrollment_controls::enrollment_s24_acl_native' -or $assignment.ordinal -ne 2)){throw 'negative only assigned S24 ordinal2'}
if((FileId 'C:\') -ne $binding.volume_root_file_id){throw 'outer parent identity'}
$parentBefore=ReadDescriptor $parent $true
if(-not $parentBefore.compliant -or $parentBefore.identity_before -ne $binding.fixture_parent_file_id){throw 'parent changed'}
if($Mode -ne 'Inspect'){
    if(Test-Path -LiteralPath $Root){throw 'collision; no adoption/retry'}
    # cmd mkdir provides absent-only creation; unlike Directory.CreateDirectory it cannot adopt.
    $cmd='C:\Windows\System32\cmd.exe'
    if((Get-FileHash -LiteralPath $cmd -Algorithm SHA256).Hash.ToLower() -cne $binding.cmd_sha256){throw 'cmd identity'}
    $create=& $cmd /d /c "mkdir $Root" 2>&1
    if($LASTEXITCODE -ne 0){throw "outer create failed; preserve: $create"}
    $created=FileId $Root
    if(@(Get-ChildItem -LiteralPath $Root -Force).Count -ne 0){throw 'nonempty newly created root'}
    if($Mode -eq 'Prepare'){
        $d=[Security.AccessControl.DirectorySecurity]::new();$d.SetOwner($u);$d.SetAccessRuleProtection($true,$false)
        $d.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($u,[Security.AccessControl.FileSystemRights]::FullControl,[Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit',[Security.AccessControl.PropagationFlags]::None,[Security.AccessControl.AccessControlType]::Allow))
        [IO.Directory]::SetAccessControl($Root,$d)
    }
    if((FileId $Root) -ne $created){throw 'created identity changed'}
}
$result=ReadDescriptor $Target ($Target -eq $Root)
$parentAfter=ReadDescriptor $parent $true
if($parentBefore.sddl -ne $parentAfter.sddl -or $parentBefore.identity_before -ne $parentAfter.identity_after){throw 'parent drift'}
if((FileId 'C:\') -ne $binding.volume_root_file_id){throw 'outer parent moved'}
$userBytes=New-Object byte[] $u.BinaryLength;$u.GetBinaryForm($userBytes,0)
$passed=if($Mode -eq 'Negative'){(-not $result.compliant) -and (-not $result.protected)}else{$result.compliant}
@{at=[DateTime]::UtcNow.ToString('o');mode=$Mode;root=$Root;target=$result;parent=$parentAfter;user=$u.Value;token_owner=(Token).Value;user_sid_hex=([BitConverter]::ToString($userBytes).Replace('-','').ToLower());passed=[bool]$passed}|ConvertTo-Json -Depth 8 -Compress
if(-not $passed){exit 2}
