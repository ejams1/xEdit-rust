// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// The oracle side of `cargo xtask parity modgroups`. The script mode of
// the GUI never activates mod groups (TfrmMain only does after the first
// load in the view and edit modes), so the script clicks the menu items of
// the main form that a user would: the scenario's action ({{MENU}}, such as
// "mniNavEditModGroup", each of which ends with a reload of the mod
// groups), or the reload alone ("mniViewModGroupsReload"), which reads the
// mod group files again, writes their validation messages to the log,
// shows the selection (the harness answers every dialog as the scenario
// says), saves the selection and activates it. Then it writes the conflict
// status of every main record of every loaded file, as conflicts.pas does,
// now with the activated mod groups (ModGroupsEnabled).
unit OracleModGroups;

function Initialize: Integer;
var
  i, j, n, singles, ca, ct: Integer;
  f, r: IInterface;
  outList, log: TStringList;
  fileName: string;
begin
  Result := 0;
  outList := TStringList.Create;
  log := TStringList.Create;
  try
    try
      AddMessage('[modgroups] action start');
      {{ACTIONS}}
      AddMessage('[modgroups] action end');
      for i := 0 to Pred(FileCount) do begin
        f := FileByIndex(i);
        outList.Add('file' + #9 + GetFileName(f) + #9 + IntToStr(GetLoadOrder(f)));
      end;
      for i := 0 to Pred(FileCount) do begin
        f := FileByIndex(i);
        fileName := GetFileName(f);
        singles := 0;
        n := RecordCount(f);
        for j := 0 to Pred(n) do begin
          r := RecordByIndex(f, j);
          ca := ConflictAllForMainRecord(r);
          ct := ConflictThisForMainRecord(r);
          // caOnlyOne with ctOnlyOne
          if (ca = 1) and (ct = 4) then
            Inc(singles)
          else
            outList.Add(fileName + #9 + IntToHex(GetLoadOrderFormID(r), 8) + #9 + Signature(r) + #9 + IntToStr(ca) + #9 + IntToStr(ct));
        end;
        outList.Add('singles' + #9 + fileName + #9 + IntToStr(singles) + #9 + IntToStr(n));
      end;
      outList.SaveToFile('{{WORK}}out\conflicts.txt');
      log.Add('done');
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
