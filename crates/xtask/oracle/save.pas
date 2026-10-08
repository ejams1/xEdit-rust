// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// The oracle side of `cargo xtask parity oracle-save`: the GUI build of
// xEdit runs this script (-script:) once the plugins are loaded and writes
// the plugin under test as its own save would: FileWriteToStream calls
// TwbFile.WriteToStream, which runs PrepareSave, as SaveChanged does. The
// harness replaces the placeholders in double braces and reads status.txt
// once done.txt appears.
unit OracleSave;

function Initialize: Integer;
var
  i: Integer;
  f: IInterface;
  fs: TFileStream;
  log: TStringList;
  found: Boolean;
begin
  Result := 0;
  found := False;
  log := TStringList.Create;
  try
    try
      for i := 0 to Pred(FileCount) do begin
        f := FileByIndex(i);
        log.Add('loaded: ' + GetFileName(f));
        if SameText(GetFileName(f), '{{FILE}}') then begin
          found := True;
          fs := TFileStream.Create('{{WORK}}out\saved', fmCreate);
          try
            FileWriteToStream(f, fs, 0);
          finally
            fs.Free;
          end;
        end;
      end;
      if found then
        log.Add('done')
      else
        log.Add('error: {{FILE}} is not loaded');
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
