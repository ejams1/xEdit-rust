{
  Parity fixture (issue #31): the element and container state surface of
  `xejviScriptAdapterElement.pas`/`xejviScriptAdapterContainer.pas` and
  `SetEditValue` of a main record.

  `IntToEsState` names the states 0-12 a script can ask for (`0`
  `esModified`, `1` `esInternalModified`, `2` `esUnsaved`, `3`
  `esSortKeyValid`, `4` `esExtendedSortKeyValid`, `5` `esHidden`, `6`
  `esParentHidden`, `7` `esParentHiddenChecked`, `8` `esNotReachable`, `9`
  `esReachable`, `10` `esTagged`, `11` `esResolving`, `12`
  `esNotSuitableToAddTo`) and answers anything else with `esDummy`;
  `TwbElement.SetElementState` includes or excludes exactly that bit. The
  probe walks 0..15 on the file, a main record and a subrecord: the read
  before the set, the set's return (whether the state was in the set), the
  read after it, the clear's return and the read after it. Each set is
  cleared again, so the walk leaves the states as they were.

  `ContainerStates` returns `Word(Container.ContainerStates)` for a
  container and an empty result for an element that is no container (the
  `Supports` fails); the file, the record, the group and the subrecord are
  probed.

  `SetEditValue` of a main record is `TwbMainRecord.SetEditValue`: with the
  edit gate open it parses the value as a FormID and hands it to
  `SetLoadOrderFormID` only while `wbDisplayLoadOrderFormID` is active,
  which no headless run sets, so the baseline is the `Exception` with
  `FormID can only be edited if wbDisplayLoadOrderFormID is active`.
}
unit ElStatesProbe;

function Flag(aValue: Boolean): string;
begin
  if aValue then
    Result := 'T'
  else
    Result := 'F';
end;

procedure Log(const aTag, aText: string);
begin
  AddMessage(aTag + ' | ' + aText);
end;

function Cs(e: IInterface): string;
begin
  try
    Result := IntToStr(ContainerStates(e));
  except on E: Exception do
    Result := 'exc=' + E.ClassName + ' ' + E.Message;
  end;
end;

procedure OneState(const aTag: string; e: IInterface; aIndex: Integer);
var
  prev: Boolean;
begin
  Log('get', aTag + ' ' + IntToStr(aIndex) + '=' + Flag(GetElementState(e, aIndex)));
  prev := SetElementState(e, aIndex);
  Log('set', aTag + ' ' + IntToStr(aIndex) + ' prev=' + Flag(prev) + ' now=' + Flag(GetElementState(e, aIndex)));
  prev := ClearElementState(e, aIndex);
  Log('clear', aTag + ' ' + IntToStr(aIndex) + ' prev=' + Flag(prev) + ' now=' + Flag(GetElementState(e, aIndex)));
end;

procedure StateRange(const aTag: string; e: IInterface);
var
  i: Integer;
begin
  for i := 0 to 15 do
    OneState(aTag, e, i);
end;

function Initialize: integer;
var
  f, r, sr, g, d, c: IInterface;
  s: string;
  i: Integer;
begin
  Result := 0;
  f := FileByIndex(0);
  r := RecordByIndex(f, 0);
  sr := ElementByIndex(r, 0);
  g := GroupBySignature(f, 'GMST');
  Log('cs', 'file=' + Cs(f));
  Log('cs', 'record=' + Cs(r));
  Log('cs', 'group=' + Cs(g));
  Log('cs', 'subrecord=' + Cs(sr));
  Log('cs', 'header-type=' + IntToStr(ElementType(sr)) + ' ' + Name(sr));
  Log('cs', 'member2=' + Cs(ElementByIndex(r, 2)) + ' type=' + IntToStr(ElementType(ElementByIndex(r, 2))) + ' ' + Name(ElementByIndex(r, 2)));
  StateRange('file', f);
  StateRange('record', r);
  StateRange('sub', sr);
  for i := 0 to 2 do
    ClearElementState(f, i);
  Log('cs', 'file-end=' + Cs(f));
  Log('edit', 'states ' + Flag(GetElementState(r, 0)) + Flag(GetElementState(r, 1)) + Flag(GetElementState(r, 2)));
  try
    SetEditValue(r, '000800');
    Log('edit', 'ok ' + IntToStr(GetLoadOrderFormID(r)));
  except on E: Exception do
    Log('edit', 'exc=' + E.ClassName + ' ' + E.Message);
  end;
  try
    SetEditValue(r, 'not-a-formid');
    Log('edit', 'ok-bad');
  except on E: Exception do
    Log('edit', 'exc-bad=' + E.ClassName + ' ' + E.Message);
  end;
  Log('edit', 'end-states ' + Flag(GetElementState(r, 0)) + Flag(GetElementState(r, 1)) + Flag(GetElementState(r, 2)));
  { `csInitOnce` (fix-numerics): a member whose value was read once keeps
    the bit after the release resets it (`DoReset` excludes `csInit` and
    `csInitDone` only). The record's own `Init` reads its editor ID
    (`mrEditorID := mrDef.GetEditorID(CurrentRec)`), so `EDID` holds it
    before the script reads anything; `DATA` holds it once its value is
    read. The second record is reached here for the first time. }
  Log('cs', 'member1=' + Cs(ElementByIndex(r, 1)) + ' ' + Name(ElementByIndex(r, 1)));
  Log('cs', 'member2-read=' + GetEditValue(ElementByIndex(r, 2)) + ' cs=' + Cs(ElementByIndex(r, 2)));
  Log('cs', 'member1-read=' + GetEditValue(ElementByIndex(r, 1)) + ' cs=' + Cs(ElementByIndex(r, 1)));
  Log('cs', 'header-read=' + GetEditValue(ElementByIndex(sr, 0)) + ' cs=' + Cs(sr));
  r := RecordByIndex(f, 1);
  Log('cs', 'record2=' + Cs(r));
  Log('cs', 'record2-member0=' + Cs(ElementByIndex(r, 0)));
  Log('cs', 'record2-member1=' + Cs(ElementByIndex(r, 1)) + ' ' + Name(ElementByIndex(r, 1)));
  Log('cs', 'record2-member2=' + Cs(ElementByIndex(r, 2)) + ' ' + Name(ElementByIndex(r, 2)));
  Log('cs', 'record2-edid=' + EditorID(r) + ' cs=' + Cs(ElementByIndex(r, 1)));
  Log('cs', 'record2-after=' + Cs(r));
  sr := ElementByIndex(r, 0);
  Log('cs', 'record2-held=' + Cs(r) + ' header=' + Cs(sr));
  s := GetEditValue(ElementByIndex(sr, 0));
  Log('cs', 'record2-header-read=' + s + ' cs=' + Cs(sr) + ' count=' + IntToStr(ElementCount(sr)));
  d := ElementByIndex(r, 2);
  Log('cs', 'record2-data-count=' + IntToStr(ElementCount(d)) + ' cs=' + Cs(d));
  c := ElementByIndex(d, 0);
  Log('cs', 'record2-data-child=' + Cs(d) + ' ' + Name(c) + ' ' + Cs(c));
  c := nil;
  Log('cs', 'record2-data-released=' + Cs(d));
  d := nil;
  sr := nil;
  Log('cs', 'record2-released=' + Cs(r));
end;

end.
