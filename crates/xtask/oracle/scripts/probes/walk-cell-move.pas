{
  Parity fixture (issue #24): the persistent cell of worldspace A moves to
  worldspace B (SetElementNativeValues 'Worldspace' -- the relocation of a
  cell between worldspaces, the block and sub-block chain of the new
  worldspace included) and its children follow it. Every processed record
  is logged, and the reloaded references and the saved bytes are compared.
}
unit WalkCellMove;

function Process(e: IInterface): integer;
begin
  AddMessage(Signature(e) + ' ' + IntToHex(GetLoadOrderFormID(e), 8));
  if (Signature(e) = 'CELL') and (GetLoadOrderFormID(e) = $00000803) then
    SetElementNativeValues(e, 'Worldspace', $0000080F);
end;

end.
