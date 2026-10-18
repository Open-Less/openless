$ErrorActionPreference = 'Stop'

# Load only the actual helper definitions; never run installation or registry code.
$tokens = $null
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile(
  (Join-Path $PSScriptRoot 'windows-ime-install-smoke.ps1'),
  [ref]$tokens,
  [ref]$parseErrors
)
if ($parseErrors) { throw ($parseErrors | Out-String) }
foreach ($name in @('Join-ProcessArguments', 'Invoke-CheckedProcess')) {
  $function = $ast.Find({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name
  }, $true)
  if (-not $function) { throw "Missing helper: $name" }
  . ([scriptblock]::Create($function.Extent.Text))
}

function Start-Process {
  param([string]$FilePath, [string]$ArgumentList, [switch]$Wait, [switch]$PassThru)
  if (-not $Wait -or -not $PassThru) { throw 'Helper must wait for the installer exit code' }
  [pscustomobject]@{ ExitCode = $script:TestExitCode }
}

$cases = @(
  @{ Code = 0; Allowed = @(0); ShouldThrow = $false },
  @{ Code = 3010; Allowed = @(0); ShouldThrow = $true },
  @{ Code = 0; Allowed = @(0, 3010); ShouldThrow = $false },
  @{ Code = 3010; Allowed = @(0, 3010); ShouldThrow = $false },
  @{ Code = 1603; Allowed = @(0, 3010); ShouldThrow = $true },
  @{ Code = 1641; Allowed = @(0, 3010); ShouldThrow = $true }
)
foreach ($case in $cases) {
  $script:TestExitCode = $case.Code
  $threw = $false
  try {
    Invoke-CheckedProcess -FilePath 'test-only.exe' -Label 'mock installer' -SuccessExitCodes $case.Allowed
  } catch {
    $threw = $true
  }
  if ($threw -ne $case.ShouldThrow) {
    throw "Wrong result for exit code $($case.Code), allowed $($case.Allowed -join ',')"
  }
}
Write-Host 'windows-ime-install-smoke.test.ps1 passed: six real-helper exit-code cases, no installation'
