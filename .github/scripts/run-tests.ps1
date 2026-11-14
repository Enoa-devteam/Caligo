[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('default', 'research')]
    [string]$Mode
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'This runner setup is for GitHub Actions only' }

# Elevated hosted runners default new files to the Administrators owner SID.
# Match the application's current-user ownership contract without changing ACLs.
# https://learn.microsoft.com/windows/win32/secauthz/owner-of-a-new-object
if (!('CaligoTestOwner' -as [type])) {
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Security.Principal;
public static class CaligoTestOwner {
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool SetTokenInformation(IntPtr token, int kind, ref IntPtr info, int size);
    public static void Set(SecurityIdentifier owner) {
        IntPtr token;
        if (!OpenProcessToken(GetCurrentProcess(), 0x80, out token))
            throw new Win32Exception(Marshal.GetLastWin32Error());
        IntPtr sid = Marshal.AllocHGlobal(owner.BinaryLength);
        try {
            byte[] bytes = new byte[owner.BinaryLength];
            owner.GetBinaryForm(bytes, 0);
            Marshal.Copy(bytes, 0, sid, bytes.Length);
            if (!SetTokenInformation(token, 4, ref sid, IntPtr.Size))
                throw new Win32Exception(Marshal.GetLastWin32Error());
        } finally { Marshal.FreeHGlobal(sid); CloseHandle(token); }
    }
}
'@
}

$identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
$originalOwner = $identity.Owner
$probe = Join-Path ([System.IO.Path]::GetTempPath()) ("caligo-owner-probe-" + [guid]::NewGuid().ToString('N'))
$testExit = 1
try {
    [CaligoTestOwner]::Set($identity.User)
    [System.IO.File]::WriteAllText($probe, 'CI owner preflight')
    $owner = (Get-Acl -LiteralPath $probe).GetOwner([System.Security.Principal.SecurityIdentifier])
    if ($owner.Value -ne $identity.User.Value) { throw 'Runner file owner does not match current user' }
    Remove-Item -LiteralPath $probe
    Write-Host 'Runner current-user file ownership preflight passed; original ACL assertions remain enabled.'
    & (Join-Path $PSScriptRoot '../../init.ps1') -Mode $Mode
    $testExit = $LASTEXITCODE
}
finally {
    [CaligoTestOwner]::Set($originalOwner)
    $identity.Dispose()
    if (Test-Path -LiteralPath $probe) { Remove-Item -LiteralPath $probe }
}
exit $testExit
