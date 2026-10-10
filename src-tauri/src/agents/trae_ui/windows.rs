use std::{
    io::Write,
    os::windows::process::CommandExt,
    process::{Command, Stdio},
};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{
    agents::{trae::TraeKind, AgentDetection},
    domain::{AppResult, CommandError},
    services::endpoint_url,
};

use super::{
    cached_custom_model_catalog, cached_model_names, endpoint_path, protocol_label, TraeModelInput,
    TraeUiSnapshot,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UiRequest<'a> {
    operation: &'a str,
    executable: String,
    kind: &'a str,
    activate: bool,
    display_name: &'a str,
    model_id: &'a str,
    protocol_label: &'a str,
    base_url: &'a str,
    endpoint: String,
    credential: &'a str,
    model_names: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UiResponse {
    ok: bool,
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    selection: String,
}

pub(super) fn snapshot(
    kind: TraeKind,
    detection: &AgentDetection,
    activate: bool,
) -> AppResult<TraeUiSnapshot> {
    let response = run(
        kind,
        detection,
        UiRequest {
            operation: "snapshot",
            executable: executable(detection)?,
            kind: kind_name(kind),
            activate,
            display_name: "",
            model_id: "",
            protocol_label: "",
            base_url: "",
            endpoint: String::new(),
            credential: "",
            model_names: cached_model_names(detection.config_path.as_deref())?
                .into_iter()
                .collect(),
        },
    )?;
    if response.selection.is_empty() {
        return Err(CommandError::new(
            "trae_model_selector_missing",
            "未读取到 Trae 当前模型选择器",
        ));
    }
    let catalog = cached_custom_model_catalog(detection.config_path.as_deref())?;
    Ok(TraeUiSnapshot {
        selection: response.selection,
        custom_models: catalog.names,
        custom_models_by_id: catalog.names_by_id,
        custom_endpoints_by_name: catalog.endpoints_by_name,
    })
}

pub(super) fn add_model(
    kind: TraeKind,
    detection: &AgentDetection,
    input: &TraeModelInput,
) -> AppResult<()> {
    let endpoint = endpoint_url(&input.base_url, endpoint_path(input.protocol))?.to_string();
    run(
        kind,
        detection,
        UiRequest {
            operation: "add",
            executable: executable(detection)?,
            kind: kind_name(kind),
            activate: true,
            display_name: &input.display_name,
            model_id: &input.model_id,
            protocol_label: protocol_label(input.protocol),
            base_url: input.base_url.trim_end_matches('/'),
            endpoint,
            credential: &input.credential,
            model_names: cached_model_names(detection.config_path.as_deref())?
                .into_iter()
                .collect(),
        },
    )?;
    Ok(())
}

pub(super) fn select_model(
    kind: TraeKind,
    detection: &AgentDetection,
    display_name: &str,
) -> AppResult<()> {
    run(
        kind,
        detection,
        UiRequest {
            operation: "select",
            executable: executable(detection)?,
            kind: kind_name(kind),
            activate: true,
            display_name,
            model_id: "",
            protocol_label: "",
            base_url: "",
            endpoint: String::new(),
            credential: "",
            model_names: cached_model_names(detection.config_path.as_deref())?
                .into_iter()
                .collect(),
        },
    )?;
    Ok(())
}

pub(super) fn delete_model(
    kind: TraeKind,
    detection: &AgentDetection,
    display_name: &str,
) -> AppResult<()> {
    run(
        kind,
        detection,
        UiRequest {
            operation: "delete",
            executable: executable(detection)?,
            kind: kind_name(kind),
            activate: true,
            display_name,
            model_id: "",
            protocol_label: "",
            base_url: "",
            endpoint: String::new(),
            credential: "",
            model_names: cached_model_names(detection.config_path.as_deref())?
                .into_iter()
                .collect(),
        },
    )?;
    Ok(())
}

fn executable(detection: &AgentDetection) -> AppResult<String> {
    detection
        .installation
        .as_ref()
        .map(|installation| installation.path.to_string_lossy().into_owned())
        .ok_or_else(|| CommandError::new("agent_not_installed", "未检测到 Trae 应用"))
}

fn kind_name(kind: TraeKind) -> &'static str {
    match kind {
        TraeKind::Code => "code",
        TraeKind::Work => "work",
    }
}

fn run(
    _kind: TraeKind,
    _detection: &AgentDetection,
    request: UiRequest<'_>,
) -> AppResult<UiResponse> {
    let request = Zeroizing::new(
        serde_json::to_vec(&request)
            .map_err(|_| CommandError::internal("无法准备 Trae 界面操作"))?,
    );
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            WINDOWS_UI_AUTOMATION,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ui_unavailable())?;
    child
        .stdin
        .take()
        .ok_or_else(ui_unavailable)?
        .write_all(&request)
        .map_err(|_| ui_unavailable())?;
    let output = child.wait_with_output().map_err(|_| ui_unavailable())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let response = stdout
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<UiResponse>(line.trim()).ok())
        .ok_or_else(ui_unavailable)?;
    if response.ok {
        Ok(response)
    } else {
        let code = if response.code.is_empty() {
            "trae_ui_unavailable"
        } else {
            &response.code
        };
        let message = if response.message.is_empty() {
            "无法操作 Trae 官方模型界面"
        } else {
            &response.message
        };
        Err(CommandError::new(code, message)
            .with_recovery("请保持 Trae 主窗口打开并关闭遮挡弹窗，然后重新切换。"))
    }
}

fn ui_unavailable() -> CommandError {
    CommandError::new("trae_ui_unavailable", "无法读取 Trae 官方模型界面")
        .with_recovery("请保持 Trae 主窗口打开并关闭遮挡弹窗，然后重新切换。")
}

// The command is static: model credentials are delivered over stdin as JSON,
// never embedded in a process command line. Windows PowerShell 5.1 ships the
// WPF UI Automation assemblies used here, so the integration adds no runtime
// download or third-party automation dependency.
const WINDOWS_UI_AUTOMATION: &str = r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ATSwitchNative {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extraInfo);
}
'@

function Result($ok, $code, $message, $selection = '') {
  [ordered]@{ ok = $ok; code = $code; message = $message; selection = $selection } |
    ConvertTo-Json -Compress
}

function Elements($root) {
  $root.FindAll(
    [System.Windows.Automation.TreeScope]::Descendants,
    [System.Windows.Automation.Condition]::TrueCondition
  )
}

function Role($element) {
  try { $element.Current.ControlType.ProgrammaticName } catch { '' }
}

function Texts($element) {
  $values = New-Object System.Collections.Generic.List[string]
  try { if ($element.Current.Name) { $values.Add($element.Current.Name) } } catch {}
  try { if ($element.Current.HelpText) { $values.Add($element.Current.HelpText) } } catch {}
  try {
    $pattern = $element.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    if ($pattern.Current.Value) { $values.Add($pattern.Current.Value) }
  } catch {}
  $values
}

function Exact($root, [string[]]$labels) {
  $matches = New-Object System.Collections.Generic.List[object]
  foreach ($element in (Elements $root)) {
    foreach ($value in (Texts $element)) {
      if ($labels -contains $value) { $matches.Add($element); break }
    }
  }
  $matches
}

function Press($element) {
  try {
    $pattern = $element.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
    $pattern.Invoke(); return $true
  } catch {}
  try {
    $pattern = $element.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern)
    $pattern.Select(); return $true
  } catch {}
  try {
    $pattern = $element.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
    $pattern.Expand(); return $true
  } catch {}
  try {
    $rect = $element.Current.BoundingRectangle
    if (-not $rect.IsEmpty) {
      [ATSwitchNative]::SetCursorPos([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2)) | Out-Null
      [ATSwitchNative]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
      [ATSwitchNative]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
      return $true
    }
  } catch {}
  $false
}

function Click-Center($element) {
  try {
    $rect = $element.Current.BoundingRectangle
    if (-not $rect.IsEmpty) {
      [ATSwitchNative]::SetCursorPos([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2)) | Out-Null
      [ATSwitchNative]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero)
      [ATSwitchNative]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
      return $true
    }
  } catch {}
  $false
}

function Press-Exact($root, [string[]]$labels) {
  $matches = @(Exact $root $labels)
  for ($index = $matches.Count - 1; $index -ge 0; $index--) {
    $current = $matches[$index]
    for ($level = 0; $level -lt 5 -and $null -ne $current; $level++) {
      if (Press $current) { return }
      try { $current = [System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($current) } catch { $current = $null }
    }
  }
  throw "CONTROL_MISSING"
}

function Wait-Exact($root, [string[]]$labels, [int]$seconds) {
  $deadline = [DateTime]::UtcNow.AddSeconds($seconds)
  while ([DateTime]::UtcNow -lt $deadline) {
    if (@(Exact $root $labels).Count -gt 0) { return }
    Start-Sleep -Milliseconds 100
  }
  throw "CONTROL_MISSING"
}

function Open-ModelMenu($root, $combo, [string[]]$selectionNames, [string]$current) {
  $label = if ($current) { @(Exact $combo @($current)) | Select-Object -First 1 } else { $null }
  if (-not (($null -ne $label -and (Click-Center $label)) -or (Click-Center $combo) -or (Press $combo))) {
    throw 'SELECTOR_MISSING'
  }
  try {
    Wait-Exact $root $selectionNames 2
  } catch {
    if (-not (($null -ne $label -and (Click-Center $label)) -or (Click-Center $combo) -or (Press $combo))) { throw 'MENU_MISSING' }
    try {
      Wait-Exact $root $selectionNames 3
    } catch {
      $fresh = Model-Combo $root @($current)
      if ($null -eq $fresh) { throw 'SELECTOR_MISSING' }
      $freshLabel = if ($current) { @(Exact $fresh @($current)) | Select-Object -First 1 } else { $null }
      if (-not (($null -ne $freshLabel -and (Click-Center $freshLabel)) -or (Click-Center $fresh) -or (Press $fresh))) { throw 'MENU_MISSING' }
      try { Wait-Exact $root $selectionNames 5 } catch { throw 'MENU_MISSING' }
    }
  }
}

function Contains-Text($root, [string[]]$fragments) {
  foreach ($element in (Elements $root)) {
    foreach ($value in (Texts $element)) {
      foreach ($fragment in $fragments) {
        if ($value -like "*$fragment*") { return $true }
      }
    }
  }
  $false
}

function Combo-Selection($root, [string[]]$modelNames) {
  foreach ($element in (Elements $root)) {
    if ((Role $element) -eq 'ControlType.ComboBox') {
      foreach ($value in (Texts $element)) {
        if ($modelNames -contains $value) { return $value }
      }
      foreach ($value in (Texts $element)) {
        if ($value -eq 'Auto' -or $value -eq 'Auto Mode') { return $value }
      }
    }
  }
  if (@(Exact $root @('添加模型', 'Add model')).Count -eq 0) {
    foreach ($element in (Elements $root)) {
      if ((Role $element) -ne 'ControlType.ComboBox') { continue }
      foreach ($value in (Texts $element)) {
        if (-not [string]::IsNullOrWhiteSpace($value)) { return $value }
      }
    }
  }
  ''
}

function Model-Combo($root, [string[]]$modelNames) {
  foreach ($element in (Elements $root)) {
    if ((Role $element) -ne 'ControlType.ComboBox') { continue }
    foreach ($value in (Texts $element)) {
      if ($value -eq 'Auto' -or $value -eq 'Auto Mode' -or $modelNames -contains $value) { return $element }
    }
  }
  if (@(Exact $root @('添加模型', 'Add model')).Count -eq 0) {
    foreach ($element in (Elements $root)) {
      if ((Role $element) -eq 'ControlType.ComboBox' -and @(Texts $element).Count -gt 0) {
        return $element
      }
    }
  }
  $null
}

function Close-ModelSettings($root) {
  $markers = @(Exact $root @('添加模型', 'Add model'))
  foreach ($marker in $markers) {
    $current = $marker
    for ($level = 0; $level -lt 4 -and $null -ne $current; $level++) {
      try { $current = [System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($current) } catch { $current = $null }
      if ($null -eq $current) { break }
      $children = $current.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
      foreach ($child in $children) {
        if ((Role $child) -ne 'ControlType.Button') { continue }
        if (@(Texts $child).Count -eq 0 -and (Press $child)) { return }
      }
    }
  }
  throw 'CONTROL_MISSING'
}

function Close-CodeSettings($root) {
  $back = @(Exact $root @('返回应用', 'Back to app'))
  for ($index = $back.Count - 1; $index -ge 0; $index--) {
    if (Click-Center $back[$index]) { return }
  }
  [System.Windows.Forms.SendKeys]::SendWait('^w')
}

function Enter-ModelCategory($root) {
  Press-Exact $root @('模型', 'Models')
  try { Wait-Exact $root @('添加模型', 'Add model') 2; return } catch {}
  $category = @(Exact $root @('模型', 'Models'))
  for ($index = $category.Count - 1; $index -ge 0; $index--) {
    if (Click-Center $category[$index]) {
      Wait-Exact $root @('添加模型', 'Add model') 15
      return
    }
  }
  throw 'CONTROL_MISSING'
}

function Dismiss-TransientOverlays($root) {
  for ($attempt = 0; $attempt -lt 3; $attempt++) {
    $button = $null
    foreach ($candidate in (Exact $root @('Close', '关闭'))) {
      if ((Role $candidate) -eq 'ControlType.Button') { $button = $candidate; break }
    }
    if ($null -eq $button -or -not (Press $button)) { return }
    Start-Sleep -Milliseconds 150
  }
}

function Open-SettingsDrawer($root) {
  try { $window = $root.Current.BoundingRectangle } catch { return $false }
  if ($window.IsEmpty -or $window.Width -le 0 -or $window.Height -le 0) { return $false }
  foreach ($element in (Elements $root)) {
    if ((Role $element) -ne 'ControlType.Button' -or @(Texts $element).Count -ne 0) { continue }
    try { $rect = $element.Current.BoundingRectangle } catch { continue }
    if ($rect.IsEmpty -or $rect.Width -gt 96 -or $rect.Height -gt 96) { continue }
    $centerX = ($rect.X + $rect.Width / 2 - $window.X) / $window.Width
    $centerY = ($rect.Y + $rect.Height / 2 - $window.Y) / $window.Height
    if ($centerX -ge 0.55 -and $centerX -le 0.80 -and $centerY -ge 0.07 -and $centerY -le 0.20) {
      if (Press $element) { return $true }
    }
  }
  $false
}

function Set-Edit($root, [string[]]$fragments, [string]$value, [int]$fallbackIndex) {
  $edits = New-Object System.Collections.Generic.List[object]
  foreach ($element in (Elements $root)) {
    if ((Role $element) -eq 'ControlType.Edit') {
      $edits.Add($element)
      foreach ($label in (Texts $element)) {
        foreach ($fragment in $fragments) {
          if ($label -like "*$fragment*") {
            try {
              $pattern = $element.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
              $pattern.SetValue($value); return
            } catch { throw "FIELD_UNWRITABLE" }
          }
        }
      }
    }
  }
  if ($fallbackIndex -lt $edits.Count) {
    try {
      $pattern = $edits[$fallbackIndex].GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
      $pattern.SetValue($value); return
    } catch { throw "FIELD_UNWRITABLE" }
  }
  throw "CONTROL_MISSING"
}

function Open-Settings($root, $request) {
  Dismiss-TransientOverlays $root
  if (@(Exact $root @('添加模型', 'Add model')).Count -gt 0) { return }
  if (@(Exact $root @('模型', 'Models')).Count -gt 0) {
    Enter-ModelCategory $root
    return
  }
  if ($request.kind -eq 'code') {
    $account = $null
    foreach ($element in (Elements $root)) {
      if ((Role $element) -ne 'ControlType.Button') { continue }
      foreach ($value in (Texts $element)) {
        if ($value -like '*免费*' -or $value -like '*Free*') { $account = $element; break }
      }
      if ($null -ne $account) { break }
    }
    if ($null -ne $account -and (Press $account)) {
      try {
        Wait-Exact $root @('设置', 'Settings') 3
        Press-Exact $root @('设置', 'Settings')
      } catch {}
    }
    try {
      Wait-Exact $root @('模型', 'Models') 5
    } catch {
      $settings = @(Exact $root @('设置', 'Settings'))
      for ($index = $settings.Count - 1; $index -ge 0; $index--) {
        if (Click-Center $settings[$index]) { break }
      }
      try { Wait-Exact $root @('模型', 'Models') 5 } catch {
        [System.Windows.Forms.SendKeys]::SendWait('^,')
      }
    }
  } else {
    $account = $null
    foreach ($element in (Elements $root)) {
      if ((Role $element) -eq 'ControlType.Button') {
        foreach ($value in (Texts $element)) {
          if ($value -like '*免费*' -or $value -like '*Free*') { $account = $element; break }
        }
      }
      if ($null -ne $account) { break }
    }
    if ($null -eq $account -or -not (Press $account)) { throw 'CONTROL_MISSING' }
    Wait-Exact $root @('设置', 'Settings') 3
    Press-Exact $root @('设置', 'Settings')
  }

  $directCategory = $false
  try { Wait-Exact $root @('模型', 'Models') 1; $directCategory = $true } catch {}
  if ($directCategory) {
    Enter-ModelCategory $root
    return
  } else {
    # Newer Trae releases keep settings categories in an unlabelled drawer.
    # Locate it relative to the app window, independent of window position and
    # display scaling, before falling back to the older category search.
    $foundCategory = $false
    if (Open-SettingsDrawer $root) {
      try {
        Wait-Exact $root @('模型', 'Models') 3
        Enter-ModelCategory $root
        $foundCategory = $true
      } catch {}
    }
    if ($foundCategory) {
      Wait-Exact $root @('添加模型', 'Add model') 15
      return
    }
    [System.Windows.Forms.SendKeys]::SendWait('^f')
    Start-Sleep -Milliseconds 150
    try {
      Set-Edit $root @('搜索', 'Search') '模型' 0
      Wait-Exact $root @('模型管理', 'Model management', 'Models', 'Model') 5
      $foundCategory = $true
    } catch {}
    if (-not $foundCategory) {
      Set-Edit $root @('搜索', 'Search') 'model' 0
      Wait-Exact $root @('模型管理', 'Model management', 'Models', 'Model') 5
    }
    Press-Exact $root @('模型管理', 'Model management', 'Models', 'Model')
  }
  Wait-Exact $root @('添加模型', 'Add model') 15
}

function Find-Process($executable) {
  $expected = [IO.Path]::GetFullPath($executable)
  foreach ($process in (Get-Process -Name ([IO.Path]::GetFileNameWithoutExtension($expected)) -ErrorAction SilentlyContinue)) {
    try {
      if ($process.MainWindowHandle -ne 0 -and [IO.Path]::GetFullPath($process.Path) -ieq $expected) { return $process }
    } catch {}
  }
  $null
}

try {
  $request = [Console]::In.ReadToEnd() | ConvertFrom-Json
  $process = Find-Process $request.executable
  if ($null -eq $process) { Result $false 'trae_not_running' 'Trae 当前未运行'; exit 0 }
  if ($request.activate) { [ATSwitchNative]::SetForegroundWindow($process.MainWindowHandle) | Out-Null; Start-Sleep -Milliseconds 250 }
  $root = [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
  if ($null -eq $root) { throw 'UI_UNAVAILABLE' }

  switch ($request.operation) {
    'snapshot' {
      $selection = Combo-Selection $root $request.modelNames
      if (-not $selection) {
        Dismiss-TransientOverlays $root
        if ($request.kind -eq 'work' -and @(Exact $root @('添加模型', 'Add model')).Count -gt 0) {
          Close-ModelSettings $root
        } elseif ($request.kind -eq 'code') {
          Close-CodeSettings $root
        }
        $deadline = [DateTime]::UtcNow.AddSeconds(15)
        while ([DateTime]::UtcNow -lt $deadline -and -not $selection) {
          Start-Sleep -Milliseconds 100
          $selection = Combo-Selection $root $request.modelNames
        }
      }
      if (-not $selection) { throw 'SELECTOR_MISSING' }
      Result $true '' '' $selection
    }
    'select' {
      [System.Windows.Forms.SendKeys]::SendWait('{ESC}')
      Start-Sleep -Milliseconds 150
      Dismiss-TransientOverlays $root
      $selectionNames = if ($request.displayName -in @('Auto', 'Auto Mode')) { @('Auto', 'Auto Mode') } else { @($request.displayName) }
      $modelNames = @($request.modelNames) + $selectionNames
      if (@(Exact $root @('添加模型', 'Add model', '返回应用', 'Back to app')).Count -gt 0) {
        if ($request.kind -eq 'work') { Close-ModelSettings $root }
        if ($request.kind -eq 'code') { Close-CodeSettings $root }
        Start-Sleep -Milliseconds 250
      }
      $current = Combo-Selection $root $modelNames
      if ($current -in $selectionNames) { Result $true '' ''; break }
      $combo = Model-Combo $root $modelNames
      $deadline = [DateTime]::UtcNow.AddSeconds(15)
      while ([DateTime]::UtcNow -lt $deadline -and $null -eq $combo) {
        Start-Sleep -Milliseconds 100
        $combo = Model-Combo $root $modelNames
      }
      if ($null -eq $combo -and $request.kind -eq 'code') {
        Close-CodeSettings $root
        $deadline = [DateTime]::UtcNow.AddSeconds(15)
        while ([DateTime]::UtcNow -lt $deadline -and $null -eq $combo) {
          Start-Sleep -Milliseconds 100
          $combo = Model-Combo $root $modelNames
        }
      }
      if ($null -eq $combo) { throw 'SELECTOR_MISSING' }
      Open-ModelMenu $root $combo $selectionNames $current
      Press-Exact $root $selectionNames
      Start-Sleep -Milliseconds 250
      if ((Combo-Selection $root $modelNames) -notin $selectionNames) {
        $candidate = @(Exact $root $selectionNames) | Select-Object -Last 1
        if ($null -eq $candidate) {
          $combo = Model-Combo $root $modelNames
          if ($null -eq $combo) { throw 'SELECTOR_MISSING' }
          Open-ModelMenu $root $combo $selectionNames ''
          $candidate = @(Exact $root $selectionNames) | Select-Object -Last 1
        }
        if ($null -eq $candidate -or -not (Click-Center $candidate)) { throw 'SELECTION_FAILED' }
        Start-Sleep -Milliseconds 250
        if ((Combo-Selection $root $modelNames) -notin $selectionNames) { throw 'SELECTION_FAILED' }
      }
      Result $true '' ''
    }
    'add' {
      Open-Settings $root $request
      Press-Exact $root @('添加模型', 'Add model')
      Wait-Exact $root @('自定义模型', 'Custom model') 3
      Press-Exact $root @('自定义模型', 'Custom model')
      Wait-Exact $root @('API 格式', 'API format') 3
      $protocols = @('OpenAI Chat Completions 格式', 'OpenAI Responses API 格式', 'Anthropic Messages 格式')
      $currentProtocol = $null
      foreach ($protocol in $protocols) {
        if (@(Exact $root @($protocol)).Count -gt 0) { $currentProtocol = $protocol; break }
      }
      if ($null -ne $currentProtocol -and $currentProtocol -ne $request.protocolLabel) {
        Press-Exact $root @($currentProtocol)
        Wait-Exact $root @($request.protocolLabel) 3
        Press-Exact $root @($request.protocolLabel)
      }
      Wait-Exact $root @('模型 ID', 'Model ID') 3
      $endpoint = if (Contains-Text $root @('完整请求 URL', 'full request URL')) { $request.endpoint } else { $request.baseUrl }
      Set-Edit $root @('api.openai.com', 'anthropic.com', 'API URL', 'Base URL') $endpoint 0
      Set-Edit $root @('模型 ID', 'Model ID') $request.modelId 1
      Set-Edit $root @('展示名称', 'Display name') $request.displayName 2
      Set-Edit $root @('API Key', 'API key') $request.credential 3
      Press-Exact $root @('添加模型', 'Add model')
      $deadline = [DateTime]::UtcNow.AddSeconds(60)
      while ([DateTime]::UtcNow -lt $deadline) {
        if (@(Exact $root @($request.displayName)).Count -gt 0) { Result $true '' ''; break }
        Start-Sleep -Milliseconds 300
      }
      if ([DateTime]::UtcNow -ge $deadline) { throw 'CONNECTIVITY_TIMEOUT' }
    }
    'delete' {
      Open-Settings $root $request
      $named = @(Exact $root @($request.displayName))
      if ($named.Count -eq 0) { Result $true '' ''; break }
      $row = $named[0]
      for ($level = 0; $level -lt 6 -and $null -ne $row; $level++) {
        if ((Role $row) -eq 'ControlType.DataItem') { break }
        $row = [System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($row)
      }
      if ($null -eq $row) { throw 'DELETE_CONTROL_MISSING' }
      $rect = $row.Current.BoundingRectangle
      [ATSwitchNative]::SetCursorPos([int]($rect.Right - 20), [int]($rect.Y + $rect.Height / 2)) | Out-Null
      Start-Sleep -Milliseconds 250
      $buttons = New-Object System.Collections.Generic.List[object]
      foreach ($element in ($row.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition))) {
        if ((Role $element) -eq 'ControlType.Button') { $buttons.Add($element) }
      }
      if ($buttons.Count -lt 2 -or -not (Press $buttons[1])) { throw 'DELETE_CONTROL_MISSING' }
      Wait-Exact $root @('删除', 'Delete') 3
      Press-Exact $root @('删除', 'Delete')
      Start-Sleep -Milliseconds 300
      if (@(Exact $root @($request.displayName)).Count -gt 0) { throw 'DELETE_FAILED' }
      Result $true '' ''
    }
    default { throw 'UI_UNAVAILABLE' }
  }
} catch {
  $token = $_.Exception.Message
  switch ($token) {
    'CONTROL_MISSING' { Result $false 'trae_ui_control_missing' 'Trae 当前界面缺少所需控件' }
    'FIELD_UNWRITABLE' { Result $false 'trae_ui_field_unwritable' 'Trae 模型表单字段无法写入' }
    'SELECTOR_MISSING' { Result $false 'trae_model_selector_missing' '未找到 Trae 模型选择器' }
    'MENU_MISSING' { Result $false 'trae_model_menu_missing' 'Trae 模型菜单未打开' }
    'SELECTION_FAILED' { Result $false 'trae_model_selection_failed' 'Trae 未确认新的模型选择' }
    'CONNECTIVITY_TIMEOUT' { Result $false 'trae_connectivity_check_timeout' '等待 Trae 自定义模型连通性测试超时' }
    'DELETE_CONTROL_MISSING' { Result $false 'trae_model_delete_control_missing' '未找到 Trae 自定义模型删除按钮' }
    'DELETE_FAILED' { Result $false 'trae_model_delete_failed' 'Trae 未确认删除 AT-Switch 管理模型' }
    default { Result $false 'trae_ui_unavailable' '无法读取 Trae 官方模型界面' }
  }
}
"#;
