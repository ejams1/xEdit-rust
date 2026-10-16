{
  Parity fixture (issue #24): two temporary references move to the other
  cell (SetElementEditValues 'Cell' -- the contained-in relocation of the
  record between the cell children groups), so one temporary children
  group empties and goes and one is created in the file while the walk
  runs. Every processed record is logged, and the saved bytes are
  compared.
}
unit WalkMove;

function Process(e: IInterface): integer;
begin
  AddMessage(Signature(e) + ' ' + IntToHex(GetLoadOrderFormID(e), 8));
  if Signature(e) = 'REFR' then begin
    if GetLoadOrderFormID(e) = $00000808 then
      SetElementEditValues(e, 'Cell', 'xFitInteriorCell')
    else if GetLoadOrderFormID(e) = $0000080A then
      SetElementEditValues(e, 'Cell', 'xFitTempCell');
  end;
end;

end.
