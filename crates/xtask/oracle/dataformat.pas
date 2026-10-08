// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// The oracle side of `cargo xtask parity nif --text`: the GUI build of
// xEdit runs this script (-script:) and, through the script adapter of the
// data format units, writes for every file of files.txt its ToText dump
// (<n>.txt), its SaveToFile (<n>.saved) or the message of its exception
// (<n>.err), and dfCalcHash of every line of names.txt (hashes.txt). The
// harness replaces the placeholders in double braces and reads status.txt
// once done.txt appears.
unit OracleDataFormat;

function Initialize: Integer;
var
  i: Integer;
  log, sl, files, names: TStringList;
  ext, path, stem: string;
  el: TdfElement;
begin
  Result := 0;
  log := TStringList.Create;
  sl := TStringList.Create;
  files := TStringList.Create;
  names := TStringList.Create;
  try
    try
      names.LoadFromFile('{{WORK}}names.txt');
      for i := 0 to Pred(names.Count) do
        sl.Add(IntToHex(dfCalcHash(names[i]), 8));
      sl.SaveToFile('{{WORK}}out\hashes.txt');

      files.LoadFromFile('{{WORK}}files.txt');
      for i := 0 to Pred(files.Count) do begin
        path := files[i];
        stem := '{{WORK}}out\' + IntToStr(i);
        ext := LowerCase(ExtractFileExt(path));
        el := nil;
        try
          if (ext = '.nif') or (ext = '.kf') then
            el := TwbNifFile.Create
          else if ext = '.bgsm' then
            el := TwbBGSMFile.Create
          else if ext = '.bgem' then
            el := TwbBGEMFile.Create
          else if ext = '.lod' then
            el := TwbLODSettingsTES5File.Create
          else if ext = '.dlodsettings' then
            el := TwbLODSettingsFO3File.Create
          else if ext = '.lst' then
            el := TwbLODTreeLSTFile.Create
          else if (ext = '.btt') or (ext = '.dtl') then
            el := TwbLODTreeBTTFile.Create
          else if ext = '.fuz' then
            el := TwbFUZFile.Create
          else
            el := TwbDDSFile.Create;
          el.LoadFromFile(path);
          sl.Text := el.ToText;
          sl.SaveToFile(stem + '.txt');
          el.SaveToFile(stem + '.saved');
        except
          on E: Exception do begin
            sl.Text := E.Message;
            sl.SaveToFile(stem + '.err');
          end;
        end;
        if Assigned(el) then
          el.Free;
      end;
      log.Add('done');
    except
      on E: Exception do
        log.Add('error: ' + E.Message);
    end;
  finally
    log.SaveToFile('{{WORK}}status.txt');
    log.Free;
    sl.Free;
    files.Free;
    names.Free;
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
