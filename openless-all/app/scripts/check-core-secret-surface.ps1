param()

$ErrorActionPreference = "Stop"
$appRoot = Split-Path -Parent $PSScriptRoot
$coreRoot = Join-Path $appRoot "crates/openless-core/src"

$surfaceFiles = @(
  Join-Path $coreRoot "api.rs"
  Join-Path $coreRoot "events.rs"
  Join-Path $coreRoot "types.rs"
)
$forbiddenPublicField = 'pub\s+[A-Za-z0-9_]*(secret|token|api_key|password|authorization|pairing_pin|pin)[A-Za-z0-9_]*\s*:'
$violations = foreach ($file in $surfaceFiles) {
  Select-String -Path $file -Pattern $forbiddenPublicField -CaseSensitive:$false |
    Where-Object {
      # QA approval_token is a session-bound UI correlation handle, not a provider credential.
      # It is intentionally serialized so the UI can submit an explicit approval.
      $_.Line -notmatch '^\s*pub\s+approval_token\s*:'
    }
}
if ($violations) {
  $violations | ForEach-Object { Write-Error "$($_.Path):$($_.LineNumber): secret-like public snapshot/event field: $($_.Line.Trim())" }
  exit 1
}

$credentials = Get-Content -Raw (Join-Path $coreRoot "credentials.rs")
$secretStart = $credentials.IndexOf("pub struct SecretValue")
$secretEnd = $credentials.IndexOf("pub trait CredentialStore")
if ($secretStart -lt 0 -or $secretEnd -le $secretStart) {
  Write-Error "SecretValue contract block was not found"
  exit 1
}
$secretBlock = $credentials.Substring($secretStart, $secretEnd - $secretStart)
if ($secretBlock -match 'Serialize|Deserialize') {
  Write-Error "SecretValue must not implement or derive serde serialization"
  exit 1
}
if ($secretBlock -notmatch '\[REDACTED\]') {
  Write-Error "SecretValue Debug output must remain redacted"
  exit 1
}

Write-Host "[ok] core snapshot/event surfaces contain no unreviewed secret-like public fields; SecretValue is non-serde and redacted"
