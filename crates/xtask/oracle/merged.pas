// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// The oracle side of `cargo xtask parity merged`. The script clicks the
// menu items of the main form a user would (the actions below: the reload of
// the mod groups when the scenario activates some, then
// "mniNavCreateMergedPatch"); the harness answers the dialogs of the
// handler (the warning of the newer games, the file name of InputQuery).
// Then it writes the loaded files in the order of FileByIndex and the
// patch as the GUI's save writes it (FileWriteToStream runs PrepareSave).
unit OracleMerged;

function Initialize: Integer;
var
  i: Integer;
  f, patch: IInterface;
  fs: TFileStream;
  outList, log: TStringList;
  found: Boolean;
begin
  Result := 0;
  found := False;
  outList := TStringList.Create;
  log := TStringList.Create;
  try
    try
      AddMessage('[merged] action start');
      {{ACTIONS}}
      AddMessage('[merged] action end');
      for i := 0 to Pred(FileCount) do begin
        f := FileByIndex(i);
        outList.Add(GetFileName(f));
        if SameText(GetFileName(f), '{{FILE}}') then begin
          patch := f;
          found := True;
        end;
      end;
      outList.SaveToFile('{{WORK}}out\files.txt');
      if found then begin
        fs := TFileStream.Create('{{WORK}}out\saved', fmCreate);
        try
          FileWriteToStream(patch, fs, 0);
        finally
          fs.Free;
        end;
        log.Add('done');
      end else
        log.Add('error: the GUI made no {{FILE}}');
    except
      on E: Exception do
        log.Add('error: ' + E.Message);
    end;
  finally
    log.SaveToFile('{{WORK}}status.txt');
    log.Free;
    outList.Free;
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
