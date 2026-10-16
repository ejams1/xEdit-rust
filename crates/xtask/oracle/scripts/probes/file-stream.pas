{
  Parity fixture (issue #31): `FileWriteToStream` of
  `xejviScriptAdapterFile.pas` -- the file's bytes into a `TStream` object
  with the `TwbResetModified` the third argument names (`0` is `rmNo`, `2`
  is `rmSetInternal`, anything else is `rmYes`).

  The probe writes the loaded master into a `TMemoryStream` four times:
  unchanged with `rmNo`, after an edit with `rmNo` (the modification stays,
  the unsaved flag goes), after another edit with `rmYes` (the modified and
  internal-modified flags go too), and after a third edit through the
  "any other value" route. Each write logs the stream's size and position
  and the first six bytes of the file (the header record `TES4` and its
  size), and the element states 0/1/2 (`esModified`, `esInternalModified`,
  `esUnsaved`) before and after, so the flags the write resets are compared
  as exactly as the bytes. A nil stream is a no-op. The probe ends with a
  completed write, so the file is not left unsaved and neither side saves
  anything at the end of the run (a `Saving:` line would be a log
  difference).
}
unit FileStreamProbe;

var
  gOut: string;

function Flag(aValue: Boolean): string;
begin
  if aValue then
    Result := 'T'
  else
    Result := 'F';
end;

procedure Log(const aTag, aText: string);
begin
  AddMessage(aTag + ' | ' + aText);
end;

function States(f: IInterface): string;
begin
  Result := 'mod=' + Flag(GetElementState(f, 0)) + ' int=' + Flag(GetElementState(f, 1)) + ' uns=' + Flag(GetElementState(f, 2));
end;

function Head(ms: TMemoryStream): string;
var
  r: TBinaryReader;
begin
  r := TBinaryReader.Create(ms);
  Result := IntToHex(r.ReadUInt32, 8) + ':' + IntToHex(r.ReadUInt16, 4);
  r.Free;
end;

function Written(const aTag: string; ms: TMemoryStream): string;
begin
  Result := aTag + ' size=' + IntToStr(ms.Size) + ' pos=' + IntToStr(ms.Position) + ' head=' + Head(ms);
end;

procedure Reset(ms: TMemoryStream);
begin
  ms.Size := 0;
  ms.Position := 0;
end;

function Initialize: integer;
var
  f, r: IInterface;
  ms: TMemoryStream;
begin
  Result := 0;
  gOut := ScriptsPath;
  f := FileByIndex(0);
  Log('file', 'name=' + GetFileName(f) + ' records=' + IntToStr(RecordCount(f)) + ' states=' + States(f));
  ms := TMemoryStream.Create;
  FileWriteToStream(f, ms, 0);
  Log('write', Written('fresh-rmNo', ms));
  Log('state', 'after-fresh-rmNo ' + States(f));
  r := RecordByIndex(f, 0);
  SetElementEditValues(r, 'EDID', 'parity-probe-a');
  Log('state', 'edited ' + States(f));
  Reset(ms);
  FileWriteToStream(f, ms, 0);
  Log('write', Written('edited-rmNo', ms));
  Log('state', 'after-edited-rmNo ' + States(f));
  SetElementEditValues(r, 'EDID', 'parity-probe-b');
  Reset(ms);
  FileWriteToStream(f, ms, 1);
  Log('write', Written('edited-rmYes', ms));
  Log('state', 'after-edited-rmYes ' + States(f));
  SetElementEditValues(r, 'EDID', 'parity-probe-c');
  Reset(ms);
  FileWriteToStream(f, ms, 2);
  Log('write', Written('edited-rmSetInternal', ms));
  Log('state', 'after-edited-rmSetInternal ' + States(f));
  SetElementEditValues(r, 'EDID', 'parity-probe-d');
  Reset(ms);
  FileWriteToStream(f, ms, 9);
  Log('write', Written('edited-rmOther', ms));
  Log('state', 'after-edited-rmOther ' + States(f));
  FileWriteToStream(f, nil, 1);
  Log('nil', 'after-nil-stream ' + States(f));
end;

end.
