{
  TStringList.SaveToFile over a plain path and over the extended-length
  spelling ExportImportTexts.pas builds (`\\?\` plus the base path), with
  and without spaces and brackets in the file name, and into a folder
  ForceDirectories made through its extended path. Every failure logs the
  exception class and message, and FileExists follows each attempt:
  ExportImportTexts.pas wraps its save in `try ... except end`, so an
  exception there is the one way its run writes no file at all.
}
unit UserScript;

var
  sl: TStringList;

procedure TrySave(const aTag, aPath: string);
begin
  try
    sl.SaveToFile(aPath);
    AddMessage(aTag + '=ok');
  except on E: Exception do
    AddMessage(aTag + '=exc ' + E.ClassName + ': ' + E.Message);
  end;
  AddMessage(aTag + '-file=' + IfThen(FileExists(aPath), 'T', 'F'));
end;

function Initialize: Integer;
var
  d, e: string;
begin
  sl := TStringList.Create;
  sl.Text := 'hello';
  d := wbTempPath + 'parity-save';
  AddMessage('force=' + IfThen(ForceDirectories(d), 'T', 'F'));
  e := '\\?\' + d;
  TrySave('plain', d + '\plain.txt');
  TrySave('ext', e + '\plain.txt');
  TrySave('ext-brackets', e + '\Skyrim.esm [0] [TES4-00000000] [0] Record Header [0] Signature.txt');
  AddMessage('force-sub=' + IfThen(ForceDirectories(e + '\Skyrim.esm [0] [TES4-00000000] [0] Record Header [0] Signature'), 'T', 'F'));
  TrySave('ext-sub', e + '\Skyrim.esm [0] [TES4-00000000] [0] Record Header [0] Signature\deep.txt');
  sl.Free;
  Result := 1;
end;

end.
