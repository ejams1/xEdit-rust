{
  Parity probe (issue #25): the conflict status the navigation tree's nodes
  hold after JumpTo, over the host-25 fixture (40 GMST records, a STAT and
  an interior CELL with two REFRs): which rows the jump classifies.
}
unit HostPaint;

procedure ShowNodes(aTag: string);
var
  i: Integer;
  s: string;
  e: IInterface;
begin
  s := '';
  for i := 0 to 39 do begin
    e := RecordByHexFormID(IntToHex($800 + i, 8));
    s := s + ' ' + VarToStr(ConflictAllForNode(e));
  end;
  AddMessage(aTag + ' GMST' + s);
  AddMessage(aTag + ' STAT ' + VarToStr(ConflictAllForNode(RecordByHexFormID('00000900'))));
  AddMessage(aTag + ' CELL ' + VarToStr(ConflictAllForNode(RecordByHexFormID('00000A00'))));
  AddMessage(aTag + ' REFR ' + VarToStr(ConflictAllForNode(RecordByHexFormID('00000A01'))) + ' ' + VarToStr(ConflictAllForNode(RecordByHexFormID('00000A02'))));
  AddMessage(aTag + ' header ' + VarToStr(ConflictAllForNode(ElementByIndex(FileByIndex(0), 0))));
end;

function Initialize: Integer;
begin
  Result := 0;
  if not Assigned(RecordByHexFormID('00000827')) then begin
    AddMessage('the host-25 fixture is not loaded');
    Exit;
  end;
  ShowNodes('start');
  JumpTo(RecordByHexFormID('00000814'), False);
  ShowNodes('jump 814');
  JumpTo(RecordByHexFormID('00000A02'), False);
  ShowNodes('jump A02');
  JumpTo(RecordByHexFormID('00000800'), False);
  ShowNodes('jump 800');
end;

end.
