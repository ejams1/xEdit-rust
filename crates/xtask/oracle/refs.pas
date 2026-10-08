// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// The oracle side of `cargo xtask parity refs`: the GUI build of xEdit
// runs this script (-script:) once the plugins are loaded and their
// reference information is built (or loaded from its cache), and writes the
// referenced-by list of every master record in the format of
// `xedit refs dump`: a line "F <name>" per loaded file in load order, then
// per master record with references "R <load order FormID>@<file> <count>"
// and the entries as "|<load order FormID>@<file>", 32 to a line. Files
// are named, not numbered: the hardcoded file has the load order of the
// game master. The output goes to {{WORK}}out\refs<n>.txt in
// parts of about 200000 lines; status.txt names the part count, and
// done.txt marks the end.
unit OracleRefs;

var
  lines: TStringList;
  part: Integer;

procedure Flush;
begin
  lines.SaveToFile('{{WORK}}out\refs' + IntToStr(part) + '.txt');
  Inc(part);
  lines.Clear;
end;

function Hex(aValue: Variant): string;
begin
  // In halves: a FormID above $7FFFFFFF does not fit the Integer of
  // IntToHex.
  Result := IntToHex(aValue div 65536, 4) + IntToHex(aValue mod 65536, 4);
end;

function Initialize: Integer;
var
  i, j, k, n: Integer;
  f, rec, r: IInterface;
  log: TStringList;
  s: string;
begin
  Result := 0;
  part := 0;
  log := TStringList.Create;
  lines := TStringList.Create;
  try
    try
      for i := 0 to Pred(FileCount) do begin
        f := FileByIndex(i);
        lines.Add('F ' + GetFileName(f));
      end;
      for i := 0 to Pred(FileCount) do begin
        f := FileByIndex(i);
        for j := 0 to Pred(RecordCount(f)) do begin
          rec := RecordByIndex(f, j);
          if not IsMaster(rec) then
            Continue;
          n := ReferencedByCount(rec);
          if n < 1 then
            Continue;
          lines.Add('R ' + Hex(GetLoadOrderFormID(rec)) + '@' + GetFileName(f) + ' ' + IntToStr(n));
          s := '';
          for k := 0 to Pred(n) do begin
            r := ReferencedByIndex(rec, k);
            s := s + '|' + Hex(GetLoadOrderFormID(r)) + '@' + GetFileName(GetFile(r));
            if (k mod 32) = 31 then begin
              lines.Add(s);
              s := '';
            end;
          end;
          if s <> '' then
            lines.Add(s);
          if lines.Count >= 200000 then
            Flush;
        end;
      end;
      Flush;
      log.Add('parts: ' + IntToStr(part));
      log.Add('done');
    except
      on E: Exception do
        log.Add('error: ' + E.Message);
    end;
  finally
    log.SaveToFile('{{WORK}}status.txt');
    log.Free;
    lines.Free;
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
