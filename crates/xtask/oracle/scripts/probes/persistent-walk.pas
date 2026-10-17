{
  Parity #41 probe: the walk of "Change persistent flag.pas" answered Yes over
  the walk-churn fixture, with every processed element logged: a reference
  that gets the Persistent flag moves to the persistent children group of
  its cell while the walk runs (the GUI's tree re-homes its node), and the
  visiting order and the re-visits show where the walk goes next. The cells
  are read through an element first, as walk-move.pas reads them: the
  release's quick init of a record named first loses its subrecords (#64),
  and the walk names the cells in the names of their references.
}
unit userscript;

function Initialize: integer;
begin
  ElementByPath(RecordByFormID(FileByIndex(0), $00000803, False), 'EDID');
  ElementByPath(RecordByFormID(FileByIndex(0), $00000804, False), 'EDID');
  ElementByPath(RecordByFormID(FileByIndex(0), $00000807, False), 'EDID');
  Result := 0;
end;

function Process(e: IInterface): integer;
var
  s: string;
begin
  s := Signature(e);
  AddMessage(IntToStr(ElementType(e)) + ' ' + Name(e));
  if (s <> 'REFR') and (s <> 'ACHR') and (s <> 'ACRE') and (s <> 'PGRE') and (s <> 'PMIS') and (s <> 'PHZD') then
    Exit;
  SetIsPersistent(e, True);
end;

end.
