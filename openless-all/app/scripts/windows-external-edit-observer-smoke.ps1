param(
  [Parameter(Mandatory = $true)]
  [string]$ExpectedPattern,

  [Parameter(Mandatory = $true)]
  [string]$ExpectedReplacement,

  [int]$TimeoutSeconds = 20,

  [string]$SummaryJsonPath = ""
)

$ErrorActionPreference = "Stop"

function Read-JsonFile($Path) {
  if (-not (Test-Path $Path)) {
    return $null
  }
  $raw = Get-Content -Raw -Encoding UTF8 -Path $Path
  if ([string]::IsNullOrWhiteSpace($raw)) {
    return $null
  }
  return $raw | ConvertFrom-Json
}

function Read-CorrectionRules($Path) {
  $json = Read-JsonFile $Path
  if ($null -eq $json) {
    return @()
  }
  if ($json -is [System.Array]) {
    return @($json)
  }
  return @($json)
}

function Get-MatchingRules($Rules, $Pattern, $Replacement) {
  return @(
    $Rules | Where-Object {
      $_.pattern -eq $Pattern -and $_.replacement -eq $Replacement
    }
  )
}

function Get-ExtEditLines($Path, $StartLine) {
  if (-not (Test-Path $Path)) {
    return @()
  }
  $lines = Get-Content -Encoding UTF8 -Path $Path
  if ($lines.Count -eq 0) {
    return @()
  }
  $effectiveStartLine = if ($StartLine -ge $lines.Count) { 0 } else { $StartLine }
  return @(
    $lines[$effectiveStartLine..($lines.Count - 1)] |
      Where-Object { $_ -match "\[extedit\]" } |
      ForEach-Object { [string]$_ }
  )
}

$dataDir = Join-Path $env:APPDATA "OpenLess"
$rulesPath = Join-Path $dataDir "correction-rules.json"
$logPath = Join-Path $env:LOCALAPPDATA "OpenLess\Logs\openless.log"

$preRules = Read-CorrectionRules $rulesPath
$preMatches = Get-MatchingRules $preRules $ExpectedPattern $ExpectedReplacement
$preMatchById = @{}
foreach ($rule in $preMatches) {
  $preMatchById[$rule.id] = [bool]$rule.enabled
}

$logStartLine = 0
if (Test-Path $logPath) {
  $logStartLine = (Get-Content -Encoding UTF8 -Path $logPath).Count
}

Write-Host "== Windows external-edit observer smoke =="
Write-Host "Expected pattern     : $ExpectedPattern"
Write-Host "Expected replacement : $ExpectedReplacement"
Write-Host "Rules path           : $rulesPath"
Write-Host "Log path             : $logPath"
Write-Host ""
Write-Host "Expected:"
Write-Host "  1. 在已支持的 Windows 外部编辑目标中完成一次正式插入。"
Write-Host "  2. 在短窗口内对同一控件中的术语做人工纠正。"
Write-Host "  3. correction-rules.json 出现新的 enabled rule，或已有同 rule 被重新启用。"
Write-Host "  4. openless.log 出现 [extedit] armed / learned / persisted 等日志。"
Write-Host ""
Write-Host "Actual:"
Write-Host "  正在轮询本地数据文件与日志，等待结果..."

$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$actualMatches = @()
$persistObserved = $false

while ((Get-Date) -lt $deadline) {
  Start-Sleep -Milliseconds 500
  $extEditLines = Get-ExtEditLines $logPath $logStartLine
  if ($extEditLines | Where-Object { $_ -match "\[extedit\] persisted rule id=" }) {
    $persistObserved = $true
    break
  }
}

$postRules = Read-CorrectionRules $rulesPath
$actualMatches = Get-MatchingRules $postRules $ExpectedPattern $ExpectedReplacement
$success = $false
foreach ($rule in $actualMatches) {
  $wasEnabled = $false
  $hadRule = $preMatchById.ContainsKey($rule.id)
  if ($hadRule) {
    $wasEnabled = [bool]$preMatchById[$rule.id]
  }
  if ((-not $hadRule) -or ((-not $wasEnabled) -and [bool]$rule.enabled)) {
    $success = $true
    break
  }
}

$extEditLines = Get-ExtEditLines $logPath $logStartLine
$summary = [ordered]@{
  expectedPattern = $ExpectedPattern
  expectedReplacement = $ExpectedReplacement
  timeoutSeconds = $TimeoutSeconds
  success = $success
  persistObserved = $persistObserved
  rulesPath = $rulesPath
  logPath = $logPath
  matchedRuleCount = @($actualMatches).Count
  matchedRules = @(
    $actualMatches | ForEach-Object {
      [ordered]@{
        id = $_.id
        enabled = [bool]$_.enabled
        createdAt = if ($null -ne $_.PSObject.Properties["createdAt"]) {
          $_.createdAt
        } else {
          $_.created_at
        }
      }
    }
  )
  exteditLogLines = $extEditLines
}

$summaryJson = $summary | ConvertTo-Json -Depth 6
if (-not [string]::IsNullOrWhiteSpace($SummaryJsonPath)) {
  $summaryDir = Split-Path -Parent $SummaryJsonPath
  if (-not [string]::IsNullOrWhiteSpace($summaryDir) -and -not (Test-Path $summaryDir)) {
    New-Item -ItemType Directory -Path $summaryDir | Out-Null
  }
  [System.IO.File]::WriteAllText($SummaryJsonPath, $summaryJson, [System.Text.UTF8Encoding]::new($false))
}

Write-Host ""
Write-Host "Summary JSON:"
Write-Output $summaryJson

if (-not $success) {
  throw "No newly learned or re-enabled rule matched '$ExpectedPattern' -> '$ExpectedReplacement' within ${TimeoutSeconds}s."
}

Write-Host "Windows external-edit observer smoke passed."
