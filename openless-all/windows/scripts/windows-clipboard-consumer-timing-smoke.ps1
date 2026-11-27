param(
  [int]$ConsumerDelayMs = 250,
  [int]$RestoreDelayMs = 150,
  [string]$InsertedText = "OPENLESS_DICTATED_TEXT",
  [string]$PreviousText = "OPENLESS_OLDER_CLIPBOARD"
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Windows.Forms
Add-Type -ReferencedAssemblies @("System.Windows.Forms") @"
using System;
using System.Threading;
using System.Windows.Forms;

public sealed class DelayedClipboardReader {
  private readonly Thread thread;
  private string observed;
  private Exception failure;

  public DelayedClipboardReader(int delayMs) {
    thread = new Thread(() => {
      try {
        Thread.Sleep(delayMs);
        if (Clipboard.ContainsText()) {
          observed = Clipboard.GetText();
        }
      } catch (Exception ex) {
        failure = ex;
      }
    });
    thread.SetApartmentState(ApartmentState.STA);
  }

  public void Start() {
    thread.Start();
  }

  public string JoinAndGetResult() {
    thread.Join();
    if (failure != null) {
      throw failure;
    }
    return observed;
  }
}
"@

function Restore-ClipboardValue($Value) {
  if ($null -eq $Value) {
    cmd /c "echo off | clip" | Out-Null
    return
  }
  Set-Clipboard -Value $Value
}

$originalClipboard = Get-Clipboard -Raw -ErrorAction SilentlyContinue
try {
  Set-Clipboard -Value $InsertedText
  $reader = [DelayedClipboardReader]::new($ConsumerDelayMs)
  $reader.Start()

  Start-Sleep -Milliseconds $RestoreDelayMs
  Restore-ClipboardValue $PreviousText
  $observedText = $reader.JoinAndGetResult()
  $result = [pscustomobject]@{
    consumerDelayMs = $ConsumerDelayMs
    restoreDelayMs = $RestoreDelayMs
    insertedText = $InsertedText
    previousText = $PreviousText
    observedText = $observedText
    matchedInserted = ($observedText -eq $InsertedText)
  }
  $result | ConvertTo-Json -Compress
} finally {
  Restore-ClipboardValue $originalClipboard
}
