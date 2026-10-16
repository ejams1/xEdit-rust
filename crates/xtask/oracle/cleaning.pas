// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// The oracle side of `cargo xtask parity clean-scenarios`: the GUI's own
// cleaning menu items over a selection of file nodes. The script mode has
// the filter variables and `ApplyFilter` of the script host (the filter
// for cleaning, as `Apply filter for cleaning.pas` applies it), and
// `frmMain`, whose components the script reaches by name: the navigation
// tree `vstNav`, whose file nodes the script selects with the mouse
// messages a click and a Ctrl+click send (`TControl.Perform`; the filter
// leaves the file nodes collapsed, one row each, in load order), and the
// menu items `mniNavUndeleteAndDisableReferences`,
// `mniNavRemoveIdenticalToMaster` and `mniNavLOManagersDirtyInfo`, whose
// `Click` runs the handler on that selection. The handlers write their
// lines to the message log of the main form, which the harness reads; the
// plugins the scenario compares are written to the `out` folder.
unit OracleCleaning;

const
  WM_KEYDOWN = $0100;
  WM_LBUTTONDOWN = $0201;
  WM_LBUTTONUP = $0202;
  MK_LBUTTON = $0001;
  MK_CONTROL = $0008;
  VK_HOME = $24;
  VK_LEFT = $25;
  VK_DOWN = $28;

var
  log: TStringList;

function FileNamed(aName: string): IInterface;
var
  i: Integer;
begin
  Result := nil;
  for i := 0 to Pred(FileCount) do
    if SameText(GetFileName(FileByIndex(i)), aName) then begin
      Result := FileByIndex(i);
      Exit;
    end;
  raise Exception.Create(aName + ' is not loaded');
end;

function RowOf(aName: string): Integer;
var
  i: Integer;
begin
  Result := -1;
  for i := 0 to Pred(FileCount) do
    if SameText(GetFileName(FileByIndex(i)), aName) then begin
      Result := i;
      Exit;
    end;
  raise Exception.Create(aName + ' is not loaded');
end;

// A click on the row of a file node, with Ctrl held when `aAdd`.
procedure ClickRow(aName: string; aAdd: Boolean);
var
  tree: TObject;
  row, h, x, y, keys: Integer;
begin
  tree := frmMain.FindComponent('vstNav');
  row := RowOf(aName);
  h := tree.DefaultNodeHeight;
  x := tree.Indent * 2 + 24;
  y := row * h + h div 2;
  keys := MK_LBUTTON;
  if aAdd then
    keys := keys or MK_CONTROL;
  AddMessage('[cleaning] select ' + aName + ' row ' + IntToStr(row));
  tree.Perform(WM_LBUTTONDOWN, keys, (y shl 16) or x);
  keys := 0;
  if aAdd then
    keys := MK_CONTROL;
  tree.Perform(WM_LBUTTONUP, keys, (y shl 16) or x);
end;

// The file nodes collapsed: from the first row, Left (collapse) and Down
// once per loaded file.
procedure CollapseFiles;
var
  tree: TObject;
  i: Integer;
begin
  tree := frmMain.FindComponent('vstNav');
  tree.Perform(WM_KEYDOWN, VK_HOME, 0);
  for i := 0 to Pred(FileCount) do begin
    tree.Perform(WM_KEYDOWN, VK_LEFT, 0);
    tree.Perform(WM_KEYDOWN, VK_DOWN, 0);
  end;
end;

function Initialize: Integer;
var
  fs: TFileStream;
begin
  Result := 0;
  log := TStringList.Create;
  try
    try
      {{ACTIONS}}
      log.Add('done');
    except
      on E: Exception do
        log.Add('error: ' + E.Message);
    end;
  finally
    log.SaveToFile('{{WORK}}status.txt');
    log.Free;
  end;
  log := TStringList.Create;
  try
    log.Add('done');
    log.SaveToFile('{{WORK}}done.txt');
  finally
    log.Free;
  end;
end;

end.
