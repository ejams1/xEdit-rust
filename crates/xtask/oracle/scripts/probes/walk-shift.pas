{
  Parity fixture (issue #24): the shape of `Worldspace displacement` on
  the fixture. Every CELL shifts its grid cell by +1, 0 and every placed
  reference shifts its DATA position by one cell, so `UpdateCellChildGroup`
  moves the references whose cell changed while the walk runs; one
  temporary reference is removed (RemoveNode). Every processed record logs
  its signature and FormID, so the re-visits of the live walk and the cell
  moves are compared line by line and the saved bytes are compared too.
}
unit WalkShift;

function Process(e: IInterface): integer;
begin
  AddMessage(Signature(e) + ' ' + IntToHex(GetLoadOrderFormID(e), 8));
  if Signature(e) = 'CELL' then
    SetElementNativeValues(e, 'XCLC\X', GetElementNativeValues(e, 'XCLC\X') + 1)
  else if (Signature(e) = 'REFR') or (Signature(e) = 'ACHR') then begin
    SetElementNativeValues(e, 'DATA\Position\X', GetElementNativeValues(e, 'DATA\Position\X') + 4096);
    if GetLoadOrderFormID(e) = $00000808 then
      RemoveNode(e);
  end;
end;

end.
