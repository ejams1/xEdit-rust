// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// The probe of `cargo xtask parity resources`: what the container handler
// holds after the load and what it finds. The harness puts the calls of a
// scenario (crates/xtask/oracle/resources/*.json) in place of the PROBES
// placeholder; the same script runs in the GUI oracle and in the port, and
// both write {{WORK}}out\resources.txt.
unit OracleResources;

var
  plog: TStringList;

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

// The containers in the order they were added.
procedure ProbeContainers;
var
  sl: TStringList;
  i: Integer;
begin
  sl := TStringList.Create;
  try
    ResourceContainerList(sl);
    plog.Add('containers ' + IntToStr(sl.Count));
    for i := 0 to Pred(sl.Count) do
      plog.Add('  ' + sl[i]);
  finally
    sl.Free;
  end;
end;

// Whether a resource is found, in which containers, and the CRC32 of the
// data the last of them gives.
procedure ProbeResource(aName: string);
var
  sl: TStringList;
  i, n: Integer;
  s: string;
begin
  sl := TStringList.Create;
  try
    s := 'resource ' + aName + ':';
    if ResourceExists(aName) then
      s := s + ' exists'
    else
      s := s + ' missing';
    n := ResourceCount(aName, sl);
    s := s + ' count ' + IntToStr(n) + ' crc ' + IntToHex(wbCRC32Resource('', aName), 8);
    plog.Add(s);
    for i := 0 to Pred(sl.Count) do
      plog.Add('  in ' + sl[i]);
  finally
    sl.Free;
  end;
end;

// The names a container lists below a folder: their count and the first
// and last ones, in the container's order.
procedure ProbeListing(aIndex: Integer; aFolder: string; aShow: Integer);
var
  containers, sl: TStringList;
  i: Integer;
begin
  containers := TStringList.Create;
  sl := TStringList.Create;
  try
    ResourceContainerList(containers);
    if aIndex >= containers.Count then begin
      plog.Add('listing ' + IntToStr(aIndex) + ' ' + aFolder + ': no container');
      Exit;
    end;
    ResourceList(containers[aIndex], sl, aFolder);
    plog.Add('listing ' + IntToStr(aIndex) + ' ' + aFolder + ': ' + IntToStr(sl.Count));
    for i := 0 to Pred(sl.Count) do
      if (i < aShow) or (i >= sl.Count - aShow) then
        plog.Add('  ' + IntToStr(i) + ' ' + sl[i]);
  finally
    sl.Free;
    containers.Free;
  end;
end;

// The text of a value as the editor shows it (a localized string resolves
// through the tables of the containers).
procedure ProbeValue(aFile, aHex, aPath: string);
begin
  plog.Add('value ' + aFile + ' ' + aHex + ' ' + aPath + ': ' + GetElementEditValues(RecordIn(aFile, aHex), aPath));
end;

// The sort key of an element, which names a file or folder hash once the
// load is done (wbFileHashCallback, wbFolderHashCallback).
procedure ProbeKey(aFile, aHex, aPath: string);
var
  el: IInterface;
begin
  el := ElementByPath(RecordIn(aFile, aHex), aPath);
  if not Assigned(el) then
    raise Exception.Create('no element ' + aPath);
  plog.Add('key ' + aFile + ' ' + aHex + ' ' + aPath + ': ' + SortKey(el, False));
end;

function Initialize: Integer;
var
  sl: TStringList;
begin
  Result := 0;
  plog := TStringList.Create;
  sl := TStringList.Create;
  try
    try
{{PROBES}}
      sl.Add('done');
    except
      on E: Exception do
        sl.Add('error: ' + E.Message);
    end;
    plog.SaveToFile('{{WORK}}out\resources.txt');
  finally
    sl.SaveToFile('{{WORK}}status.txt');
    sl.Free;
    plog.Free;
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
