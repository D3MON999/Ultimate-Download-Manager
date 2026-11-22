# Signs one file with the project's code-signing certificate, when one is set up; otherwise
# leaves it unsigned and says so (the build goes on). The build calls it for the program, the
# browser helper and the installer.
#
# Set up one of these, as environment variables of the build:
#   A certificate in the Windows certificate store (a USB token's too):
#     UDM_SIGN_THUMBPRINT   its SHA-1 thumbprint
#   A .pfx file:
#     UDM_SIGN_PFX          its path
#     UDM_SIGN_PFX_PASSWORD its password
#   Azure Trusted Signing:
#     UDM_SIGN_DLIB         path to Azure.CodeSigning.Dlib.dll
#     UDM_SIGN_METADATA     path to the metadata.json naming the account and profile

param([Parameter(Mandatory = $true)][string]$File)

$ErrorActionPreference = "Stop"
$thumb = $env:UDM_SIGN_THUMBPRINT
$pfx = $env:UDM_SIGN_PFX
$dlib = $env:UDM_SIGN_DLIB

if (-not $thumb -and -not $pfx -and -not $dlib) {
    Write-Host "Not signed (no code-signing certificate set up): $File"
    exit 0
}

$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
if (-not $signtool) {
    Write-Error "signtool.exe not found: install the Windows SDK."
}

$timestamp = "http://timestamp.digicert.com"
if ($dlib) {
    & $signtool sign /v /fd SHA256 /tr "http://timestamp.acs.microsoft.com" /td SHA256 /dlib $dlib /dmdf $env:UDM_SIGN_METADATA $File
} elseif ($thumb) {
    & $signtool sign /sha1 $thumb /fd SHA256 /tr $timestamp /td SHA256 $File
} else {
    & $signtool sign /f $pfx /p $env:UDM_SIGN_PFX_PASSWORD /fd SHA256 /tr $timestamp /td SHA256 $File
}
exit $LASTEXITCODE
