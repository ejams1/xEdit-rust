{
  Parity #35 probe: the order the sort of a topic's responses gives
  (TwbGroupRecord.Sort under wbSortINFO, ProcessDIAL, DoInsertRecord) and
  the INOM and INOA lists it gives the topic. For Fallout 3's HELLO topic
  (DIAL 000000D2) in each loaded file: the responses of its children group
  in the group's order with the PNAM each links to, read twice, the entries
  of the topic's lists (the first read of the topic's subrecords builds it,
  and a topic without its lists sorts its group then), and the group again.
}
unit userscript;

function PNAMText(aRecord: IInterface): string;
var
  e: IInterface;
begin
  e := ElementBySignature(aRecord, 'PNAM');
  if not Assigned(e) then
    Result := '-'
  else
    Result := IntToHex(GetNativeValue(e), 8);
end;

procedure ReportGroup(aLabel: string; aGroup: IInterface);
var
  i: Integer;
  s: string;
  r: IInterface;
begin
  s := '';
  for i := 0 to Pred(ElementCount(aGroup)) do begin
    r := ElementByIndex(aGroup, i);
    s := s + ' ' + IntToHex(GetLoadOrderFormID(r), 8) + '<' + PNAMText(r);
  end;
  AddMessage(aLabel + ' (' + IntToStr(ElementCount(aGroup)) + '):' + s);
end;

procedure ReportList(aRecord: IInterface; aSignature: string);
var
  i: Integer;
  s: string;
  l: IInterface;
begin
  l := ElementBySignature(aRecord, aSignature);
  if not Assigned(l) then begin
    AddMessage(aSignature + ': none');
    Exit;
  end;
  s := '';
  for i := 0 to Pred(ElementCount(l)) do
    s := s + ' ' + IntToHex(GetNativeValue(ElementByIndex(l, i)), 8);
  AddMessage(aSignature + ' (' + IntToStr(ElementCount(l)) + '):' + s);
end;

function Initialize: Integer;
var
  i: Integer;
  f, r, g: IInterface;
begin
  for i := 0 to Pred(FileCount) do begin
    f := FileByIndex(i);
    r := RecordByFormID(f, $000000D2, False);
    if not Assigned(r) then
      Continue;
    if not Equals(GetFile(r), f) then
      Continue;
    AddMessage('[' + GetFileName(f) + '] ' + Name(r));
    g := ChildGroup(r);
    if not Assigned(g) then begin
      AddMessage('no children group');
      Continue;
    end;
    ReportGroup('first read', g);
    ReportGroup('second read', g);
    ReportList(r, 'INOM');
    ReportList(r, 'INOA');
    ReportGroup('after the lists', g);
  end;
  Result := 0;
end;

end.
