{
  DirectoryExists and ForceDirectories over a plain and an extended-length
  path, with and without the trailing backslash. `ExportImportTexts.pas`
  prefixes its base path with `\\?\` and then tests `DirectoryExists` on
  `ExtractFilePath`, which ends with a backslash: the probe records which
  of the two spellings the release considers a directory.
}
unit UserScript;

function Initialize: Integer;
var
  d, e: string;
begin
  d := wbTempPath + 'parity-dir';
  AddMessage('plain-no-trailing=' + IfThen(DirectoryExists(d), 'T', 'F'));
  AddMessage('force-plain=' + IfThen(ForceDirectories(d), 'T', 'F'));
  AddMessage('plain-after=' + IfThen(DirectoryExists(d), 'T', 'F'));
  AddMessage('plain-trailing=' + IfThen(DirectoryExists(d + '\'), 'T', 'F'));
  e := '\\?\' + d;
  AddMessage('ext-no-trailing=' + IfThen(DirectoryExists(e), 'T', 'F'));
  AddMessage('ext-trailing=' + IfThen(DirectoryExists(e + '\'), 'T', 'F'));
  AddMessage('force-ext=' + IfThen(ForceDirectories(e + '\sub'), 'T', 'F'));
  AddMessage('ext-sub-no-trailing=' + IfThen(DirectoryExists(e + '\sub'), 'T', 'F'));
  AddMessage('ext-sub-trailing=' + IfThen(DirectoryExists(e + '\sub\'), 'T', 'F'));
  Result := 1;
end;

end.
