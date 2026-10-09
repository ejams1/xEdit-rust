// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// The oracle side of `cargo xtask parity oracle-edit`: the harness puts
// the steps of an edit sequence (crates/xtask/oracle/edits/*.json),
// translated into calls of the xEdit script API, in place of the STEPS
// placeholder, and the writes of the plugins to compare in place of the
// SAVES placeholder. Each command does what the session command of the
// same name does: the record of a FormID is the one the named file sees
// (RecordByFormID with the file's FormID), a copy adds the required
// masters silently first (AddRequiredElementMasters), as the GUI's "Copy
// as override" does after its question, and a FormID change updates the
// referencing records in editable files, as mniNavChangeFormIDClick does.
unit OracleEdit;

var
  log: TStringList;

{{HELPERS}}
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

function RecordIn(aFileName, aHex: string): IInterface;
var
  f: IInterface;
begin
  f := FileNamed(aFileName);
  Result := RecordByFormID(f, LoadOrderFormIDtoFileFormID(f, StrToInt('$' + aHex)), True);
  if not Assigned(Result) then
    raise Exception.Create('no record with FormID ' + aHex + ' seen from ' + aFileName);
end;

function OwnRecord(aFileName, aHex: string): IInterface;
begin
  Result := RecordIn(aFileName, aHex);
  if not Equals(GetFile(Result), FileNamed(aFileName)) then
    raise Exception.Create(aFileName + ' has no record with FormID ' + aHex);
end;

function PathElement(aRecord: IInterface; aPath: string): IInterface;
begin
  Result := ElementByPath(aRecord, aPath);
  if not Assigned(Result) then
    raise Exception.Create(Name(aRecord) + ' has no element at ' + aPath);
end;

// The referencing records of a record's master, collected before the
// change as mniNavChangeFormIDClick collects them.
function ReferencingRecords(r: IInterface): TList;
var
  m: IInterface;
  i: Integer;
begin
  Result := TList.Create;
  m := MasterOrSelf(r);
  for i := 0 to Pred(ReferencedByCount(m)) do
    Result.Add(ReferencedByIndex(m, i));
end;

// ShowChangeReferencedBy: nothing when no referencing record is editable;
// otherwise the editable ones, or all of them when silent.
procedure UpdateReferencing(refs: TList; oldID, newID: Cardinal; silent: Boolean);
var
  i: Integer;
  anyEditable: Boolean;
  e: IInterface;
begin
  anyEditable := False;
  for i := 0 to Pred(refs.Count) do
    if IsEditable(ObjectToElement(refs[i])) then
      anyEditable := True;
  if not anyEditable then
    Exit;
  for i := 0 to Pred(refs.Count) do begin
    e := ObjectToElement(refs[i]);
    if silent or IsEditable(e) then
      CompareExchangeFormID(e, oldID, newID);
  end;
end;

procedure ChangeFormID(r: IInterface; newID: Cardinal; silent: Boolean);
var
  refs, overrides: TList;
  m: IInterface;
  oldID: Cardinal;
  i: Integer;
begin
  oldID := GetLoadOrderFormID(r);
  refs := ReferencingRecords(r);
  overrides := TList.Create;
  try
    if silent then begin
      m := MasterOrSelf(r);
      for i := 0 to Pred(OverrideCount(m)) do
        overrides.Add(OverrideByIndex(m, i));
    end;
    if oldID <> newID then begin
      SetLoadOrderFormID(r, newID);
      for i := 0 to Pred(overrides.Count) do
        SetLoadOrderFormID(ObjectToElement(overrides[i]), newID);
    end;
    UpdateReferencing(refs, oldID, newID, silent);
  finally
    refs.Free;
    overrides.Free;
  end;
end;

// mniNavRenumberFormIDsFromClick with the file as its own target and a
// start object ID: the new records in FormID order take the FormIDs from
// the start, keeping the ones already in the new range.
procedure Renumber(aFileName: string; aStart: Cardinal);
var
  f, r: IInterface;
  baseID, startID, endID, oldID, newID, highID: Cardinal;
  records, taken: TList;
  targets: TStringList;
  i, j: Integer;
begin
  f := FileNamed(aFileName);
  baseID := StrToInt('$' + GetLoadOrderFileID(f)) shl 24;
  records := TList.Create;
  taken := TList.Create;
  targets := TStringList.Create;
  try
    for i := 0 to Pred(RecordCount(f)) do begin
      r := RecordByIndex(f, i);
      if (GetLoadOrderFormID(r) and $FF000000) = baseID then
        records.Add(r);
    end;
    if records.Count = 0 then
      Exit;
    startID := baseID or aStart;
    endID := startID + records.Count;
    highID := endID;
    for i := 0 to records.Count do
      taken.Add(nil);
    for i := 0 to Pred(records.Count) do begin
      oldID := GetLoadOrderFormID(ObjectToElement(records[i]));
      if (oldID >= startID) and (oldID <= endID) then
        taken[oldID - startID] := records[i];
    end;
    j := 0;
    for i := 0 to Pred(records.Count) do begin
      oldID := GetLoadOrderFormID(ObjectToElement(records[i]));
      if (oldID >= startID) and (oldID <= endID) then
        targets.Add('')
      else begin
        while (j < taken.Count) and Assigned(taken[j]) do
          Inc(j);
        targets.Add(IntToHex(startID + j, 8));
        Inc(j);
      end;
    end;
    for i := 0 to Pred(records.Count) do
      if targets[i] <> '' then
        ChangeFormID(ObjectToElement(records[i]), StrToInt('$' + targets[i]), True);
    SetElementNativeValues(ElementByIndex(f, 0), 'HEDR\Next Object ID', (highID + 1) and $FFFFFF);
  finally
    records.Free;
    taken.Free;
    targets.Free;
  end;
end;

function Initialize: Integer;
var
  f, r, el: IInterface;
  sl: TStringList;
  fs: TFileStream;
begin
  Result := 0;
  log := TStringList.Create;
  try
    try
{{STEPS}}
{{SAVES}}
      log.Add('done');
    except
      on E: Exception do
        log.Add('error: ' + E.Message);
    end;
  finally
    log.SaveToFile('{{WORK}}status.txt');
    log.Free;
  end;
  sl := TStringList.Create;
  try
    sl.Add('done');
    sl.SaveToFile('{{WORK}}done.txt');
  finally
    sl.Free;
  end;
end;

end.
