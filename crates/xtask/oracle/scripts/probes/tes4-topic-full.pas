{
  Parity #41 probe: the topic of an Oblivion response as "Oblivion - Export
  Dialogues.pas" reads it (LinksTo of the response's Topic element) and the
  FULL and DATA the script prints for it, which the release exports empty
  and the port did not: the elements of the topic the script sees, by name,
  and the two values. The script reads the topic's EditorID first (for the
  voice file name); a second topic is read through its FULL first.
}
unit userscript;

procedure Report(aLabel: string; r: IInterface);
var
  i: Integer;
  s: string;
begin
  if not Assigned(r) then begin
    AddMessage(aLabel + ': nil');
    Exit;
  end;
  s := '';
  for i := 0 to Pred(ElementCount(r)) do
    s := s + ' [' + Name(ElementByIndex(r, i)) + ']';
  AddMessage(aLabel + ': ' + Name(r) + ' (' + IntToStr(ElementCount(r)) + '):' + s);
  AddMessage(aLabel + ' FULL=' + GetElementEditValues(r, 'FULL') + ' DATA=' + GetElementEditValues(r, 'DATA')
    + ' EDID=' + EditorID(r));
end;

function Initialize: Integer;
var
  f, e, t, e2, t2: IInterface;
begin
  f := FileByIndex(0);
  if GetFileName(f) <> 'Oblivion.esm' then
    f := FileByIndex(1);
  // The second topic, read through its FULL before anything else.
  e2 := RecordByFormID(f, $00146852, False);
  t2 := LinksTo(ElementByName(e2, 'Topic'));
  AddMessage('second topic FULL first: FULL=' + GetElementEditValues(t2, 'FULL') + ' DATA='
    + GetElementEditValues(t2, 'DATA') + ' EDID=' + EditorID(t2));
  e := RecordByFormID(f, $001778E3, False);
  AddMessage('response EditorID of the topic first: ' + EditorID(LinksTo(ElementByName(e, 'Topic'))));
  AddMessage('response: ' + Name(e));
  t := ElementByName(e, 'Topic');
  AddMessage('Topic element: ' + Name(t) + ' = ' + GetEditValue(t));
  Report('topic', LinksTo(t));
  Report('topic by FormID', RecordByFormID(f, GetLoadOrderFormID(LinksTo(t)), False));
  Result := 0;
end;

end.
