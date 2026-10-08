// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// The oracle side of `cargo xtask parity conflicts`: the GUI build of
// xEdit runs this script (-script:) once the plugins are loaded and writes
// the conflict status of every main record of every loaded file, as
// ConflictAllForMainRecord and ConflictThisForMainRecord (which call
// TfrmMain.ConflictLevelForMainRecord) report it. The records are visited
// in load order and, inside a file, in the order of RecordByIndex, which
// is the order the port's conflicts.list walks them in; the order matters
// for the GMST and DFOB records, whose status is cached on the first
// record of a group. A record that is the only one of its FormID
// (Single Record) is counted per file instead of listed. The harness
// reads out\conflicts.txt once done.txt appears.
unit OracleConflicts;

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
