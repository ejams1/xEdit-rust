{
  Parity fixture (issue #24): the Process walk of the changing navigation
  tree. Every processed record logs its signature and FormID, so the
  visitation order and any re-visit are compared line by line. One
  temporary reference of the temporary cell is removed (RemoveNode), and
  the log shows which record the walk re-visits after a removal.
}
unit WalkRemove;

function Process(e: IInterface): integer;
begin
  AddMessage(Signature(e) + ' ' + IntToHex(GetLoadOrderFormID(e), 8));
  if (Signature(e) = 'REFR') and (GetLoadOrderFormID(e) = $00000808) then
    RemoveNode(e);
end;

end.
