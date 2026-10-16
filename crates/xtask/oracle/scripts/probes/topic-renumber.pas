{
  Parity fixture (issue #24): the head of the topic's response chain gets a
  new FormID (SetLoadOrderFormID), so the PNAM of the following responses
  updates through the references and the topic children group sorts its
  responses again (the INFO sort of a FormID change). The chain order and
  every PNAM are logged before and after, and every processed record is
  logged, so a re-visit of the renamed record shows the new FormID.
}
unit TopicRenumber;

var
  First: Boolean;

procedure LogChain(const aTag: string);
var
  grp, rec, dial: IInterface;
  i: integer;
begin
  dial := RecordByFormID(FileByIndex(0), $00000801, False);
  grp := ChildGroup(dial);
  AddMessage(aTag + ' order ' + IntToStr(ElementCount(grp)));
  for i := 0 to Pred(ElementCount(grp)) do begin
    rec := ElementByIndex(grp, i);
    AddMessage('  ' + Name(rec) + ' PNAM ' + IntToHex(GetElementNativeValues(rec, 'PNAM'), 8));
  end;
end;

function Initialize: integer;
begin
  First := True;
  LogChain('before:');
end;

function Process(e: IInterface): integer;
begin
  AddMessage(Signature(e) + ' ' + IntToHex(GetLoadOrderFormID(e), 8));
  if (Signature(e) = 'INFO') and (GetLoadOrderFormID(e) = $0000080C) and First then begin
    First := False;
    SetLoadOrderFormID(e, $00000810);
  end;
end;

function Finalize: integer;
begin
  LogChain('after:');
end;

end.
