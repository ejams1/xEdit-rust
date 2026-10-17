{
  Parity fixture (issue #24): two temporary references move to the other
  cell (SetElementEditValues 'Cell' -- the contained-in relocation of the
  record between the cell children groups), so one temporary children
  group empties and goes and one is created in the file while the walk
  runs. Every processed record is logged, and the saved bytes are
  compared. The cells are read through an element before the walk names
  them: the release's quick init of a record named first loses its
  subrecords (#64, not reproduced by the port), which emptied both cells in
  the release's save (parity #41).
}
unit WalkMove;

function Initialize: integer;
begin
  ElementByPath(RecordByFormID(FileByIndex(0), $00000807, False), 'EDID');
  ElementByPath(RecordByFormID(FileByIndex(0), $00000804, False), 'EDID');
  Result := 0;
end;

function Process(e: IInterface): integer;
begin
  AddMessage(Signature(e) + ' ' + IntToHex(GetLoadOrderFormID(e), 8));
  if Signature(e) = 'REFR' then begin
    if GetLoadOrderFormID(e) = $00000808 then
      SetElementEditValues(e, 'Cell', Name(RecordByFormID(FileByIndex(0), $00000807, False)))
    else if GetLoadOrderFormID(e) = $0000080A then
      SetElementEditValues(e, 'Cell', Name(RecordByFormID(FileByIndex(0), $00000804, False)));
  end;
end;

end.
