{
  Parity probe (issue #25): the conflict helpers of xejviScriptAdapterHost.pas
  over the probe master -- ConflictAllForNode/ConflictThisForNode of the
  file, group and record nodes before and after the record functions, a
  JumpTo and a filter, ConflictAllForMainRecord/ConflictThisForMainRecord and
  ConflictAllForElements with two elements and with a list.
}
unit HostConflicts;

procedure ShowNode(aTag: string; e: IInterface);
begin
  AddMessage(aTag + ' ' + Name(e) + ' all ' + VarToStr(ConflictAllForNode(e)) + ' this ' + VarToStr(ConflictThisForNode(e)));
end;

procedure ShowAll(aTag: string);
var
  f, g, r: IInterface;
begin
  f := FileByIndex(0);
  ShowNode(aTag + ' file', f);
  ShowNode(aTag + ' exe', FileByIndex(1));
  g := GroupBySignature(f, 'GMST');
  ShowNode(aTag + ' group', g);
  r := RecordByHexFormID('00000800');
  ShowNode(aTag + ' record', r);
  // The other record's row: the jump paints the focused row, and whether
  // the GUI's window shows (and paints) the next row depends on its size;
  // the probe reads it where the filter classified it.
  if aTag <> 'jumped' then
    ShowNode(aTag + ' other record', RecordByHexFormID('00000801'));
  ShowNode(aTag + ' header', ElementByIndex(f, 0));
  ShowNode(aTag + ' subrecord', ElementByPath(r, 'EDID'));
end;

function Initialize: Integer;
var
  r, s: IInterface;
  lst: TList;
begin
  Result := 0;
  ShowAll('start');
  r := RecordByHexFormID('00000800');
  s := RecordByHexFormID('00000801');
  AddMessage('ConflictAllForMainRecord ' + IntToStr(ConflictAllForMainRecord(r)) + ' ConflictThisForMainRecord ' + IntToStr(ConflictThisForMainRecord(r)));
  ShowAll('main record');
  AddMessage('ConflictAllForElements same ' + IntToStr(ConflictAllForElements(r, r, False, False)));
  AddMessage('ConflictAllForElements pair ' + IntToStr(ConflictAllForElements(r, s, False, False)));
  AddMessage('ConflictAllForElements sibling ' + IntToStr(ConflictAllForElements(r, s, True, False)));
  AddMessage('ConflictAllForElements injected ' + IntToStr(ConflictAllForElements(r, s, False, True)));
  AddMessage('ConflictAllForElements data ' + IntToStr(ConflictAllForElements(ElementByPath(r, 'DATA'), ElementByPath(s, 'DATA'), False, False)));
  AddMessage('ConflictAllForElements edid ' + IntToStr(ConflictAllForElements(ElementByPath(r, 'EDID'), ElementByPath(s, 'EDID'), False, False)));
  AddMessage('ConflictAllForElements nil ' + IntToStr(ConflictAllForElements(nil, s, False, False)));
  lst := TList.Create;
  lst.Add(r);
  lst.Add(s);
  AddMessage('ConflictAllForElements list ' + IntToStr(ConflictAllForElements(lst, False, False)));
  lst.Clear;
  AddMessage('ConflictAllForElements empty list ' + IntToStr(ConflictAllForElements(lst, False, False)));
  lst.Free;
  AddMessage('ConflictAllForElements nil list ' + IntToStr(ConflictAllForElements(nil, False, False)));
  JumpTo(r, False);
  ShowAll('jumped');
  // The filter classifies the nodes it keeps; the port applies it when
  // the call that asked for it returns, so the nodes are read by the next
  // call into the script.
  FilterConflictAll := True;
  FilterConflictAllSet := [caOnlyOne];
  ApplyFilter;
end;

function Process(e: IInterface): Integer;
begin
  ShowAll('filtered');
  Result := 1;
end;

end.
