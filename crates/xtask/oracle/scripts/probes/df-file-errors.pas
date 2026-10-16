{
  Parity fixture (issue #31): the exception classes, messages and OS texts
  of the data-format file adapters, caught so that one probe covers all of
  them.

  The four routes differ upstream: `LoadFromFile` opens with `CreateFile`
  and raises `RaiseLastOSError` (an `EOSError`, `System Error. Code: <n>.`
  and the system text), `LoadFromJSONFile` and `SaveToJSONFile` go through
  `TStringList.LoadFromFile` / `TStringStream.SaveToFile` and
  `SaveToFile` through `TFileStream.Create(fmCreate)`, which raise
  `EFOpenError` with the `Cannot open file`/`Cannot create file` text. The
  empty name (where the RTL and the port's own file APIs pick different
  Win32 errors) and a missing folder are probed for each. The exception's
  class and message are logged; the uncaught location line is
  `df-load-missing.pas`/`df-save-missing.pas`'s.
}
unit DfFileErrorsProbe;

procedure Log(const aTag, aText: string);
begin
  AddMessage(aTag + ' | ' + aText);
end;

procedure TryLoad(const aTag, aPath: string);
var
  doc: TwbDDSFile;
begin
  doc := TwbDDSFile.Create;
  try
    doc.LoadFromFile(aPath);
    Log(aTag, 'ok');
  except on E: Exception do
    Log(aTag, 'exc=' + E.ClassName + ' ' + E.Message);
  end;
end;

procedure TrySave(const aTag, aPath: string);
var
  doc: TwbDDSFile;
begin
  doc := TwbDDSFile.Create;
  try
    doc.SaveToFile(aPath);
    Log(aTag, 'ok');
  except on E: Exception do
    Log(aTag, 'exc=' + E.ClassName + ' ' + E.Message);
  end;
end;

procedure TryLoadJson(const aTag, aPath: string);
var
  doc: TwbBGSMFile;
begin
  doc := TwbBGSMFile.Create;
  try
    doc.LoadFromJSONFile(aPath);
    Log(aTag, 'ok');
  except on E: Exception do
    Log(aTag, 'exc=' + E.ClassName + ' ' + E.Message);
  end;
end;

procedure TrySaveJson(const aTag, aPath: string);
var
  doc: TwbBGSMFile;
begin
  doc := TwbBGSMFile.Create;
  try
    doc.SaveToJSONFile(aPath, True);
    Log(aTag, 'ok');
  except on E: Exception do
    Log(aTag, 'exc=' + E.ClassName + ' ' + E.Message);
  end;
end;

function Initialize: integer;
begin
  Result := 0;
  TryLoad('load-empty', '');
  TryLoad('load-missing', 'C:\xedit-parity-no-such-file.dds');
  TryLoad('load-nodir', 'C:\xedit-parity-no-dir\nu\probe.dds');
  TrySave('save-empty', '');
  TrySave('save-nodir', 'C:\xedit-parity-no-dir\nu\probe.dds');
  TryLoadJson('loadjson-empty', '');
  TryLoadJson('loadjson-missing', 'C:\xedit-parity-no-such-file.json');
  TrySaveJson('savejson-empty', '');
  TrySaveJson('savejson-nodir', 'C:\xedit-parity-no-dir\nu\probe.json');
end;

end.
