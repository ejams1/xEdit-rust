// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// The oracle side of `cargo xtask parity conflicts --record <FormID>`: for
// each record named in the RECORDS placeholder (load order FormIDs), the
// conflict status of the record and its first override, and the elements
// of the two walked side by side by index with their extended sort keys
// and the ConflictAll of the pair (ConflictAllForElements, which runs
// ConflictLevelForChildNodeDatas or ConflictLevelForNodeDatas on the two).
// A tool to find where the port's comparison parts from the oracle's; the
// harness prints out\probe.txt.
unit OracleConflictsProbe;

var
  outList: TStringList;

function Text(el: IInterface; extended: Boolean): string;
begin
  if Assigned(el) then
    Result := SortKey(el, extended)
  else
    Result := '<none>';
end;

procedure Walk(m, o: IInterface; aPath: string; depth: Integer);
var
  i, n: Integer;
  me, oe, named: IInterface;
  ca: Integer;
begin
  n := 0;
  if Assigned(m) then
    n := ElementCount(m);
  if Assigned(o) and (ElementCount(o) > n) then
    n := ElementCount(o);
  for i := 0 to Pred(n) do begin
    me := nil;
    oe := nil;
    if Assigned(m) and (i < ElementCount(m)) then
      me := ElementByIndex(m, i);
    if Assigned(o) and (i < ElementCount(o)) then
      oe := ElementByIndex(o, i);
    named := me;
    if not Assigned(named) then
      named := oe;
    if Assigned(me) and Assigned(oe) then
      ca := ConflictAllForElements(me, oe, False, False)
    else
      ca := -1;
    outList.Add(aPath + '\' + IntToStr(i) + ' ' + DisplayName(named) + #9 + IntToStr(ca) + #9 + Text(me, True) + #9 + Text(oe, True));
    if (Assigned(me) or Assigned(oe)) and (depth < 8) then
      Walk(me, oe, aPath + '\' + IntToStr(i), depth + 1);
  end;
end;

function Initialize: Integer;
var
  i: Integer;
  ids: TStringList;
  r, m, o: IInterface;
  log: TStringList;
begin
  Result := 0;
  outList := TStringList.Create;
  log := TStringList.Create;
  ids := TStringList.Create;
  try
    try
      ids.Delimiter := ',';
      ids.DelimitedText := '{{RECORDS}}';
      for i := 0 to Pred(ids.Count) do begin
        r := RecordByFormID(FileByIndex(Pred(FileCount)), StrToInt('$' + ids[i]), True);
        if not Assigned(r) then begin
          outList.Add('record ' + ids[i] + ': not found');
          Continue;
        end;
        m := MasterOrSelf(r);
        outList.Add('record ' + ids[i] + ' ' + Name(m) + ': ' + IntToStr(ConflictAllForMainRecord(m)) + '/' + IntToStr(ConflictThisForMainRecord(m)) + ', overrides ' + IntToStr(OverrideCount(m)));
        if OverrideCount(m) > 0 then begin
          o := OverrideByIndex(m, 0);
          outList.Add('  override in ' + GetFileName(GetFile(o)) + ': ' + IntToStr(ConflictAllForMainRecord(o)) + '/' + IntToStr(ConflictThisForMainRecord(o)) + ', pair ' + IntToStr(ConflictAllForElements(m, o, False, False)));
          Walk(m, o, '', 0);
        end;
      end;
      outList.SaveToFile('{{WORK}}out\probe.txt');
      log.Add('done');
    except
      on E: Exception do
        log.Add('error: ' + E.Message);
    end;
  finally
    log.SaveToFile('{{WORK}}status.txt');
    log.Free;
    ids.Free;
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
